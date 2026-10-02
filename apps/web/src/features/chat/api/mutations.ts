import { useMutation, useQueryClient } from '@tanstack/react-query'
import { extractMentions } from '../lib/mentionTokens'
import { hasReaction, toggleReaction } from '../lib/reactions'
import { useChatContext } from './chatContext'
import type { ChannelInput, ChatClient, SendMessageInput, UploadOptions } from './client'
import { patchMessage, updateOutbox } from './events'
import { chatKeys } from './keys'
import { requireClient } from './queries'
import type { Attachment, Category, Conversation, ConversationState, Message, NotifyLevel, ThreadState } from './types'

const TYPING_INTERVAL = 8000
/** When the typing signal was last sent, for each composer (conversation or thread). */
const typingSentAt = new Map<string, number>()
const typingKey = (conversationId: string, threadRootId?: string | null) => `${conversationId}:${threadRootId ?? ''}`

/**
 * A write without an optimistic step. The cache changes through the events that the client reports for the write,
 * before the write resolves. The result is for the caller only: it is not written to the cache, because by then the
 * cache can hold a newer record from the live socket.
 */
function useChatMutation<TInput, TResult>(run: (client: ChatClient, input: TInput) => Promise<TResult>) {
  const { client } = useChatContext()
  return useMutation<TResult, Error, TInput>({ mutationFn: (input) => run(requireClient(client), input) })
}

export interface SendInput {
  conversationId: string
  threadRootId?: string | null
  /** Stored form: run the composer text through `encodeMentions` first. */
  body: string
  attachments?: Attachment[]
  alsoInChannel?: boolean
}

/** The row shown until the server confirms. Its id sorts after every confirmed message, and in send order. */
function optimisticMessage(input: SendMessageInput, authorId: string): Message {
  const now = Date.now()
  const threadRootId = input.threadRootId ?? null
  const mentions = extractMentions(input.body)
  return {
    id: `~${String(now).padStart(15, '0')}-${input.nonce}`,
    conversationId: input.conversationId,
    threadRootId,
    kind: 'message',
    authorId,
    body: input.body,
    mentions: threadRootId ? { ...mentions, channel: false, here: false } : mentions,
    createdAt: now,
    editedAt: null,
    deleted: false,
    attachments: input.attachments ?? [],
    reactions: [],
    pinned: false,
    alsoInChannel: Boolean(threadRootId && input.alsoInChannel),
    nonce: input.nonce,
    replyCount: 0,
    lastReplyAt: null,
    replyUserIds: [],
    lastReply: null,
    sendState: 'sending',
  }
}

/**
 * Optimistic send. The message goes to the outbox with `sendState: 'sending'` and shows at once. The confirmed one
 * (matched by nonce) takes its place, or it turns `'failed'` and stays until `retry` or `discard`.
 */
export function useSendMessage() {
  const { client, workspaceId, currentUserId } = useChatContext()
  const queryClient = useQueryClient()
  const mutation = useMutation<Message, Error, SendMessageInput, Message>({
    mutationFn: (input) => requireClient(client).sendMessage(input),
    onMutate: (input) => {
      const optimistic = optimisticMessage(input, currentUserId ?? '')
      // A retry has the nonce of its failed row and takes its place.
      updateOutbox(queryClient, workspaceId, (waiting) => [...waiting.filter((item) => item.nonce !== input.nonce), optimistic])
      // The typing signal ends with the message, so the next keystroke sends a new one.
      typingSentAt.delete(typingKey(input.conversationId, input.threadRootId))
      return optimistic
    },
    // Usually the `message.created` event took the row out already. It did not if nobody listened to the client then
    // (the user was in a different workspace), or if a resync came in place of the event.
    onSuccess: (_message, input) => updateOutbox(queryClient, workspaceId, (waiting) => waiting.filter((item) => item.nonce !== input.nonce)),
    onError: (_error, _input, optimistic) => {
      if (!optimistic) return
      updateOutbox(queryClient, workspaceId, (waiting) =>
        waiting.map((item) => (item.id === optimistic.id ? { ...item, sendState: 'failed' } : item)),
      )
    },
  })
  return {
    /** Returns the nonce, which is also the row's key (`messageKey`). */
    send: (input: SendInput): string => {
      const nonce = crypto.randomUUID()
      mutation.mutate({ ...input, nonce })
      return nonce
    },
    /** Sends a failed message again, under the same nonce. */
    retry: (message: Message) => {
      if (!message.nonce) return
      const { conversationId, threadRootId, body, attachments, alsoInChannel, nonce } = message
      mutation.mutate({ conversationId, threadRootId, body, attachments, alsoInChannel, nonce })
    },
    /** Removes a failed message. */
    discard: (message: Message) => {
      updateOutbox(queryClient, workspaceId, (waiting) => waiting.filter((item) => item.id !== message.id))
    },
  }
}

export function useEditMessage() {
  return useChatMutation(
    (client, input: { messageId: string; body: string }) => client.editMessage(input.messageId, input.body))
}

/** The lists change through the client's events: a root with replies stays as a deleted row, anything else goes. */
export function useDeleteMessage() {
  return useChatMutation((client, messageId: string) => client.deleteMessage(messageId))
}

/**
 * A change to one field of a message, shown at once and taken back if the write fails. Both steps change the message
 * that is in the cache at that moment, so an edit or a reaction that came from somebody else meanwhile stays.
 */
function useOptimisticMessageMutation<TInput extends { message: Message }>(
  run: (client: ChatClient, input: TInput, currentUserId: string) => Promise<Message>,
  change: (message: Message, input: TInput, currentUserId: string) => Message,
  revert: (message: Message, input: TInput, currentUserId: string) => Message,
) {
  const { client, workspaceId, currentUserId } = useChatContext()
  const queryClient = useQueryClient()
  const patch = (input: TInput, update: typeof change) => {
    const { conversationId, id: messageId, threadRootId } = input.message
    patchMessage(queryClient, workspaceId, { conversationId, messageId, threadRootId }, (message) => update(message, input, currentUserId ?? ''))
  }
  return useMutation<Message, Error, TInput>({
    mutationFn: (input) => run(requireClient(client), input, currentUserId ?? ''),
    onMutate: (input) => patch(input, change),
    onError: (_error, input) => patch(input, revert),
  })
}

export function useToggleReaction() {
  type Input = { message: Message; emoji: string }
  /** The user's reaction on or off. Nothing changes if it is in that state already. */
  const set = (on: (input: Input, userId: string) => boolean) => (message: Message, input: Input, userId: string) =>
    hasReaction(message.reactions, input.emoji, userId) === on(input, userId)
      ? message
      : { ...message, reactions: toggleReaction(message.reactions, input.emoji, userId) }
  // What the click asks for comes from the message as the user saw it.
  const wanted = (input: Input, userId: string) => !hasReaction(input.message.reactions, input.emoji, userId)
  return useOptimisticMessageMutation(
    (client, input: Input, userId) => client.setReaction(input.message.id, input.emoji, wanted(input, userId)),
    set(wanted),
    set((input, userId) => !wanted(input, userId)),
  )
}

export function useSetPinned() {
  return useOptimisticMessageMutation(
    (client, input: { message: Message; pinned: boolean }) => client.setPinned(input.message.id, input.pinned),
    (message, { pinned }) => ({ ...message, pinned }),
    (message, { pinned }) => ({ ...message, pinned: !pinned }),
  )
}

/**
 * A change to one of the user's own conversation settings, shown at once and taken back if the write fails. Only that
 * setting goes back: the counts of the state can have changed meanwhile.
 */
function useOptimisticStateMutation<TInput extends { conversationId: string }, TKey extends 'notify' | 'favorite'>(
  run: (client: ChatClient, input: TInput) => Promise<ConversationState>,
  key: TKey,
  value: (input: TInput) => ConversationState[TKey],
) {
  const { client, workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  const set = (conversationId: string, next: ConversationState[TKey]) =>
    queryClient.setQueryData<ConversationState[]>(chatKeys.states(workspaceId), (states) =>
      states?.map((state) => (state.conversationId === conversationId ? { ...state, [key]: next } : state)),
    )
  return useMutation<ConversationState, Error, TInput, ConversationState[TKey] | undefined>({
    mutationFn: (input) => run(requireClient(client), input),
    onMutate: (input) => {
      const previous = queryClient
        .getQueryData<ConversationState[]>(chatKeys.states(workspaceId))
        ?.find((state) => state.conversationId === input.conversationId)?.[key]
      set(input.conversationId, value(input))
      return previous
    },
    onError: (_error, input, previous) => {
      if (previous !== undefined) set(input.conversationId, previous)
    },
  })
}

export function useSetNotify() {
  return useOptimisticStateMutation(
    (client, input: { conversationId: string; notify: NotifyLevel }) => client.setNotify(input.conversationId, input.notify),
    'notify',
    (input) => input.notify,
  )
}

export function useSetFavorite() {
  return useOptimisticStateMutation(
    (client, input: { conversationId: string; favorite: boolean }) => client.setFavorite(input.conversationId, input.favorite),
    'favorite',
    (input) => input.favorite,
  )
}

export function useMarkRead() {
  return useChatMutation((client, conversationId: string) => client.markRead(conversationId))
}

/** Moves the read cursor to just before a message (for a thread reply: the thread's cursor). */
export function useMarkUnread() {
  return useChatMutation((client, messageId: string) => client.markUnread(messageId))
}

export function useMarkThreadRead() {
  return useChatMutation((client, rootId: string) => client.markThreadRead(rootId))
}

/** Resolves with the previous states: pass them to `useRestoreRead` for Undo. */
export function useMarkAllRead() {
  return useChatMutation((client, _input: void) => client.markAllRead())
}

export function useRestoreRead() {
  return useChatMutation((client, previous: { states: ConversationState[]; threads: ThreadState[] }) => client.restoreRead(previous))
}

export function useSetThreadFollow() {
  return useChatMutation(
    (client, input: { rootId: string; following: boolean }) => client.setThreadFollow(input.rootId, input.following))
}

export function useCreateChannel() {
  return useChatMutation((client, input: ChannelInput) => client.createChannel(input))
}

export function useUpdateChannel() {
  return useChatMutation(
    (client, input: { conversationId: string; patch: Partial<ChannelInput> }) => client.updateChannel(input.conversationId, input.patch))
}

export function useArchiveChannel() {
  return useChatMutation((client, conversationId: string) => client.archiveChannel(conversationId))
}

export function useJoinChannel() {
  return useChatMutation((client, conversationId: string) => client.joinChannel(conversationId))
}

export function useLeaveChannel() {
  return useChatMutation((client, conversationId: string) => client.leaveChannel(conversationId))
}

export function useAddMembers() {
  return useChatMutation(
    (client, input: { conversationId: string; userIds: string[] }) => client.addMembers(input.conversationId, input.userIds))
}

/** The lists change through the client's events (removing yourself from a private channel removes the conversation). */
export function useRemoveMember() {
  return useChatMutation((client, input: { conversationId: string; userId: string }) =>
    client.removeMember(input.conversationId, input.userId),
  )
}

/** Resolves with the DM for exactly these members (and the user): the existing one, or a new one. */
export function useOpenDm() {
  return useChatMutation((client, userIds: string[]) => client.openDm(userIds))
}

export function useCreateCategory() {
  return useChatMutation((client, name: string) => client.createCategory(name))
}

export function useRenameCategory() {
  return useChatMutation((client, input: { categoryId: string; name: string }) => client.renameCategory(input.categoryId, input.name))
}

export function useDeleteCategory() {
  return useChatMutation((client, categoryId: string) => client.deleteCategory(categoryId))
}

/** A category dropped before the category `beforeId`, or at the end. The sidebar shows it there at once. */
export function usePlaceCategory() {
  const { client, workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  const key = chatKeys.categories(workspaceId)
  return useMutation<void, Error, { categoryId: string; beforeId: string | null }>({
    mutationFn: (input) => requireClient(client).placeCategory(input.categoryId, input.beforeId),
    onMutate: ({ categoryId, beforeId }) => {
      queryClient.setQueryData<Category[]>(key, (categories) => {
        const moved = categories?.find((category) => category.id === categoryId)
        if (!categories || !moved) return categories
        const order = categories.filter((category) => category.id !== categoryId).sort((a, b) => a.position - b.position)
        const at = order.findIndex((category) => category.id === beforeId)
        order.splice(at === -1 ? order.length : at, 0, moved)
        return order.map((category, position) => ({ ...category, position }))
      })
    },
    onError: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

/**
 * A channel dropped into a category (`null`: no category), before the channel `beforeId` or at the end. The sidebar
 * shows it there at once; the server's events then give every channel of the category its position.
 */
export function usePlaceChannel() {
  const { client, workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  const key = chatKeys.conversations(workspaceId)
  return useMutation<void, Error, { conversationId: string; categoryId: string | null; beforeId: string | null }>({
    mutationFn: (input) => requireClient(client).placeChannel(input.conversationId, input.categoryId, input.beforeId),
    onMutate: ({ conversationId, categoryId, beforeId }) => {
      queryClient.setQueryData<Conversation[]>(key, (conversations) => {
        if (!conversations) return conversations
        const siblings = conversations.filter((item) => item.kind !== 'dm' && item.categoryId === categoryId && item.id !== conversationId)
        const before = siblings.find((item) => item.id === beforeId)
        // Between two whole positions, or after the last one.
        const position = before ? before.position - 0.5 : Math.max(-1, ...siblings.map((item) => item.position)) + 1
        return conversations.map((item) => (item.id === conversationId ? { ...item, categoryId, position } : item))
      })
    },
    onError: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** Not a mutation: the composer tracks each file's progress and abort signal itself. */
export function useUploadAttachment() {
  const { client } = useChatContext()
  return async (file: File, options?: UploadOptions): Promise<Attachment> => requireClient(client).uploadAttachment(file, options)
}

/** Call on every composer change with text: the signal goes out at most once every 8 seconds for each composer. */
export function useSendTyping() {
  const { client } = useChatContext()
  return (conversationId: string, threadRootId: string | null = null) => {
    const key = typingKey(conversationId, threadRootId)
    const now = Date.now()
    if (!client || now - (typingSentAt.get(key) ?? 0) < TYPING_INTERVAL) return
    typingSentAt.set(key, now)
    client.sendTyping(conversationId, threadRootId)
  }
}
