import { type QueryClient, useMutation, useQueryClient } from '@tanstack/react-query'
import { extractMentions } from '../lib/mentionTokens'
import { hasReaction, toggleReaction } from '../lib/reactions'
import { useChatContext } from './chatContext'
import type { ChannelInput, ChatClient, SendMessageInput, UploadOptions } from './client'
import { applyChatEvent, patchMessage, upsertMessage } from './events'
import { chatKeys } from './keys'
import { requireClient } from './queries'
import type { Attachment, ChatEvent, Conversation, ConversationState, Message, NotifyLevel, ThreadState } from './types'

const TYPING_INTERVAL = 8000
/** When the typing signal was last sent, for each composer (conversation or thread). */
const typingSentAt = new Map<string, number>()
const typingKey = (conversationId: string, threadRootId?: string | null) => `${conversationId}:${threadRootId ?? ''}`

interface Cache {
  queryClient: QueryClient
  workspaceId: string
  apply: (event: ChatEvent) => void
}

/**
 * A write without an optimistic step. The client reports every change as an event; applying the result here as well
 * (`onSuccess`) is idempotent and makes the caller independent of when the event arrives.
 */
function useChatMutation<TInput, TResult>(
  run: (client: ChatClient, input: TInput) => Promise<TResult>,
  onSuccess?: (result: TResult, cache: Cache) => void,
) {
  const { client, workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  return useMutation<TResult, Error, TInput>({
    mutationFn: (input) => run(requireClient(client), input),
    onSuccess: (result) =>
      onSuccess?.(result, { queryClient, workspaceId, apply: (event) => applyChatEvent(queryClient, workspaceId, event) }),
  })
}

const applyMessage = (message: Message, cache: Cache) => cache.apply({ type: 'message.updated', message })
const applyState = (state: ConversationState, cache: Cache) => cache.apply({ type: 'state.changed', state })
const applyThreadState = (state: ThreadState, cache: Cache) => cache.apply({ type: 'thread.changed', state })
const applyConversation = (conversation: Conversation, cache: Cache) => cache.apply({ type: 'conversation.changed', conversation })

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
 * Optimistic send. The message shows at once with `sendState: 'sending'`, is replaced by the confirmed one (matched by
 * nonce), or turns `'failed'` and stays until `retry` or `discard`.
 */
export function useSendMessage() {
  const { client, workspaceId, currentUserId } = useChatContext()
  const queryClient = useQueryClient()
  const mutation = useMutation<Message, Error, SendMessageInput, Message>({
    mutationFn: (input) => requireClient(client).sendMessage(input),
    onMutate: (input) => {
      const optimistic = optimisticMessage(input, currentUserId ?? '')
      upsertMessage(queryClient, workspaceId, optimistic)
      // The typing signal ends with the message, so the next keystroke sends a new one.
      typingSentAt.delete(typingKey(input.conversationId, input.threadRootId))
      return optimistic
    },
    onSuccess: (message) => applyChatEvent(queryClient, workspaceId, { type: 'message.created', message }),
    onError: (_error, _input, optimistic) => {
      if (!optimistic) return
      const { conversationId, id: messageId, threadRootId } = optimistic
      patchMessage(queryClient, workspaceId, { conversationId, messageId, threadRootId }, (message) => ({
        ...message,
        sendState: 'failed',
      }))
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
      const { conversationId, id: messageId, threadRootId } = message
      patchMessage(queryClient, workspaceId, { conversationId, messageId, threadRootId }, () => null)
    },
  }
}

export function useEditMessage() {
  return useChatMutation(
    (client, input: { messageId: string; body: string }) => client.editMessage(input.messageId, input.body),
    applyMessage,
  )
}

/** The lists change through the client's events: a root with replies stays as a deleted row, anything else goes. */
export function useDeleteMessage() {
  return useChatMutation((client, messageId: string) => client.deleteMessage(messageId))
}

/** A message change shown at once and put back if the write fails. */
function useOptimisticMessageMutation<TInput extends { message: Message }>(
  run: (client: ChatClient, input: TInput, currentUserId: string) => Promise<Message>,
  optimistic: (input: TInput, currentUserId: string) => Message,
) {
  const { client, workspaceId, currentUserId } = useChatContext()
  const queryClient = useQueryClient()
  const apply = (message: Message) => applyChatEvent(queryClient, workspaceId, { type: 'message.updated', message })
  return useMutation<Message, Error, TInput>({
    mutationFn: (input) => run(requireClient(client), input, currentUserId ?? ''),
    onMutate: (input) => apply(optimistic(input, currentUserId ?? '')),
    onSuccess: apply,
    onError: (_error, input) => apply(input.message),
  })
}

export function useToggleReaction() {
  return useOptimisticMessageMutation(
    (client, input: { message: Message; emoji: string }, userId) =>
      client.setReaction(input.message.id, input.emoji, !hasReaction(input.message.reactions, input.emoji, userId)),
    ({ message, emoji }, userId) => ({ ...message, reactions: toggleReaction(message.reactions, emoji, userId) }),
  )
}

export function useSetPinned() {
  return useOptimisticMessageMutation(
    (client, input: { message: Message; pinned: boolean }) => client.setPinned(input.message.id, input.pinned),
    ({ message, pinned }) => ({ ...message, pinned }),
  )
}

/** A change to the user's own conversation settings, shown at once and put back if the write fails. */
function useOptimisticStateMutation<TInput extends { conversationId: string }>(
  run: (client: ChatClient, input: TInput) => Promise<ConversationState>,
  optimistic: (state: ConversationState, input: TInput) => ConversationState,
) {
  const { client, workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  const apply = (state: ConversationState) => applyChatEvent(queryClient, workspaceId, { type: 'state.changed', state })
  return useMutation<ConversationState, Error, TInput, ConversationState | undefined>({
    mutationFn: (input) => run(requireClient(client), input),
    onMutate: (input) => {
      const previous = queryClient
        .getQueryData<ConversationState[]>(chatKeys.states(workspaceId))
        ?.find((state) => state.conversationId === input.conversationId)
      if (previous) apply(optimistic(previous, input))
      return previous
    },
    onSuccess: apply,
    onError: (_error, _input, previous) => {
      if (previous) apply(previous)
    },
  })
}

export function useSetNotify() {
  return useOptimisticStateMutation(
    (client, input: { conversationId: string; notify: NotifyLevel }) => client.setNotify(input.conversationId, input.notify),
    (state, { notify }) => ({ ...state, notify }),
  )
}

export function useSetFavorite() {
  return useOptimisticStateMutation(
    (client, input: { conversationId: string; favorite: boolean }) => client.setFavorite(input.conversationId, input.favorite),
    (state, { favorite }) => ({ ...state, favorite }),
  )
}

export function useMarkRead() {
  return useChatMutation((client, conversationId: string) => client.markRead(conversationId), applyState)
}

/** Moves the read cursor to just before a message (for a thread reply: the thread's cursor). */
export function useMarkUnread() {
  return useChatMutation((client, messageId: string) => client.markUnread(messageId), applyState)
}

export function useMarkThreadRead() {
  return useChatMutation((client, rootId: string) => client.markThreadRead(rootId), applyThreadState)
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
    (client, input: { rootId: string; following: boolean }) => client.setThreadFollow(input.rootId, input.following),
    applyThreadState,
  )
}

export function useCreateChannel() {
  return useChatMutation((client, input: ChannelInput) => client.createChannel(input), applyConversation)
}

export function useUpdateChannel() {
  return useChatMutation(
    (client, input: { conversationId: string; patch: Partial<ChannelInput> }) => client.updateChannel(input.conversationId, input.patch),
    applyConversation,
  )
}

export function useArchiveChannel() {
  return useChatMutation((client, conversationId: string) => client.archiveChannel(conversationId))
}

export function useJoinChannel() {
  return useChatMutation((client, conversationId: string) => client.joinChannel(conversationId), applyConversation)
}

export function useLeaveChannel() {
  return useChatMutation((client, conversationId: string) => client.leaveChannel(conversationId))
}

export function useAddMembers() {
  return useChatMutation(
    (client, input: { conversationId: string; userIds: string[] }) => client.addMembers(input.conversationId, input.userIds),
    applyConversation,
  )
}

/** The lists change through the client's events (removing yourself from a private channel removes the conversation). */
export function useRemoveMember() {
  return useChatMutation((client, input: { conversationId: string; userId: string }) =>
    client.removeMember(input.conversationId, input.userId),
  )
}

/** Resolves with the DM for exactly these members (and the user): the existing one, or a new one. */
export function useOpenDm() {
  return useChatMutation((client, userIds: string[]) => client.openDm(userIds), applyConversation)
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

/** One step up or down: a category among categories, a channel among the channels of its category. */
export function useMoveChatItem() {
  return useChatMutation(
    (client, input: { target: { categoryId: string } | { conversationId: string }; direction: 'up' | 'down' }) =>
      client.move(input.target, input.direction),
  )
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
