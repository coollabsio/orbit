import { apiClient, type createApiClient } from '@/api/client'
import {
  addChatMembers,
  addChatReaction,
  archiveChatChannel,
  createChatCategory,
  createChatChannel,
  deleteChatCategory,
  deleteChatMessage,
  editChatMessage,
  followChatThread,
  getChatThread,
  joinChatChannel,
  leaveChatChannel,
  listChatCategories,
  listChatConversations,
  listChatMessages,
  listChatPins,
  listChatStates,
  listChatThreads,
  listFollowedChatThreads,
  markChatMessageUnread,
  moveChatItem,
  openChatDm,
  pinChatMessage,
  readAllChat,
  readChatConversation,
  readChatThread,
  removeChatMember,
  removeChatReaction,
  renameChatCategory,
  restoreChatRead,
  sendChatMessage,
  updateChatChannel,
  updateChatState,
} from '@/api/generated/sdk.gen'
import type { ChatEvent as WireEvent } from '@/api/generated/types.gen'
import type { ChatClient } from './client'
import { ChatError, type ChatEvent, type ConversationState } from './types'
import { toChatError, toConversation, toEvent, toFollowedThread, toMessage, toMessagePage, toState, toThreadPage, toThreadState } from './wire'

type ApiClient = ReturnType<typeof createApiClient>

export interface HttpChatClientOptions {
  workspaceId: string
  currentUserId: string
  client?: ApiClient
  /** A "mark as read" for one conversation goes out at most once in this time; default 1000. `0` turns it off. */
  readIntervalMs?: number
}

/**
 * Chat on the Orbit server, for one workspace and the signed-in user. A write's response lists the events that the
 * write caused for the user; they go to the subscribers, so the cache changes the same way for the user's own writes
 * and (through the live socket) for everybody else's.
 */
export function createHttpChatClient(options: HttpChatClientOptions): ChatClient {
  const { workspaceId, currentUserId: me, client = apiClient } = options
  const readIntervalMs = options.readIntervalMs ?? 1000
  const listeners = new Set<(event: ChatEvent) => void>()
  const path = { workspace_id: workspaceId }
  const at = <T extends object>(ids: T) => ({ client, path: { ...path, ...ids }, throwOnError: true as const })

  const emit = (event: ChatEvent) => {
    for (const listener of [...listeners]) listener(event)
  }

  /** The body of a successful call; a failure as a `ChatError`. */
  async function call<T>(request: Promise<{ data: T | undefined }>): Promise<T> {
    try {
      const { data } = await request
      if (data === undefined) throw new ChatError('offline', 'The server sent an empty response.')
      return data
    } catch (error) {
      throw toChatError(error)
    }
  }

  /** A write: its events go to the subscribers before the caller gets the result. */
  async function write<T extends { events: WireEvent[] }>(request: Promise<{ data: T | undefined }>): Promise<T> {
    const data = await call(request)
    for (const event of data.events) emit(toEvent(event, me))
    return data
  }

  /**
   * "Mark as read" is called for every message that arrives in an open conversation. One request for each
   * conversation in `readIntervalMs` is enough: calls in between share the next request.
   */
  const reads = new Map<string, { last: number; next: Promise<ConversationState> | null }>()
  const sendRead = async (conversationId: string) =>
    toState((await write(readChatConversation(at({ conversation_id: conversationId })))).result)

  return {
    async listConversations() {
      const records = await call(listChatConversations(at({})))
      return records.map((record) => toConversation(record, me))
    },
    listCategories: () => call(listChatCategories(at({}))),
    async listStates() {
      return (await call(listChatStates(at({})))).map(toState)
    },
    async listMessages(conversationId, cursor = {}) {
      return toMessagePage(await call(listChatMessages({ ...at({ conversation_id: conversationId }), query: cursor })))
    },
    async getThread(rootId, cursor = {}) {
      return toThreadPage(await call(getChatThread({ ...at({ root_id: rootId }), query: cursor })))
    },
    async listThreads(conversationId) {
      return (await call(listChatThreads(at({ conversation_id: conversationId })))).map(toMessage)
    },
    async listFollowedThreads() {
      return (await call(listFollowedChatThreads(at({})))).map(toFollowedThread)
    },
    async listPins(conversationId) {
      return (await call(listChatPins(at({ conversation_id: conversationId })))).map(toMessage)
    },
    // Files, search and presence come with the upload, search and live-socket work.
    listFiles: async () => [],
    searchMessages: async () => ({ items: [], cursor: null }),
    getPresence: async () => [me],

    async sendMessage(input) {
      const body = {
        body: input.body,
        thread_root_id: input.threadRootId ?? null,
        also_in_channel: input.alsoInChannel ?? false,
        nonce: input.nonce,
      }
      return toMessage((await write(sendChatMessage({ ...at({ conversation_id: input.conversationId }), body }))).result)
    },
    async editMessage(messageId, body) {
      return toMessage((await write(editChatMessage({ ...at({ message_id: messageId }), body: { body } }))).result)
    },
    async deleteMessage(messageId) {
      await write(deleteChatMessage(at({ message_id: messageId })))
    },
    async setReaction(messageId, emoji, on) {
      const request = (on ? addChatReaction : removeChatReaction)(at({ message_id: messageId, emoji }))
      return toMessage((await write(request)).result)
    },
    async setPinned(messageId, pinned) {
      return toMessage((await write(pinChatMessage({ ...at({ message_id: messageId }), body: { pinned } }))).result)
    },

    markRead(conversationId) {
      const entry = reads.get(conversationId) ?? { last: 0, next: null }
      reads.set(conversationId, entry)
      if (entry.next) return entry.next
      const wait = entry.last + readIntervalMs - Date.now()
      if (wait <= 0) {
        entry.last = Date.now()
        return sendRead(conversationId)
      }
      entry.next = new Promise<void>((resolve) => setTimeout(resolve, wait)).then(() => {
        entry.next = null
        entry.last = Date.now()
        return sendRead(conversationId)
      })
      return entry.next
    },
    async markUnread(messageId) {
      return toState((await write(markChatMessageUnread(at({ message_id: messageId })))).result)
    },
    async markThreadRead(rootId) {
      return toThreadState((await write(readChatThread(at({ root_id: rootId })))).result)
    },
    async markAllRead() {
      const { result } = await write(readAllChat(at({})))
      return { states: result.states.map(toState), threads: result.threads.map(toThreadState) }
    },
    async restoreRead(previous) {
      const body = {
        states: previous.states.map((state) => ({ conversation_id: state.conversationId, last_read_message_id: state.lastReadMessageId })),
        threads: previous.threads.map((state) => ({ root_id: state.rootId, last_read_reply_id: state.lastReadReplyId })),
      }
      await write(restoreChatRead({ ...at({}), body }))
    },
    async setNotify(conversationId, notify) {
      return toState((await write(updateChatState({ ...at({ conversation_id: conversationId }), body: { notify } }))).result)
    },
    async setFavorite(conversationId, favorite) {
      return toState((await write(updateChatState({ ...at({ conversation_id: conversationId }), body: { favorite } }))).result)
    },
    async setThreadFollow(rootId, following) {
      return toThreadState((await write(followChatThread({ ...at({ root_id: rootId }), body: { following } }))).result)
    },

    async createChannel(input) {
      const body = { name: input.name, topic: input.topic ?? '', category_id: input.categoryId ?? null, kind: input.kind, member_ids: input.memberIds ?? [] }
      return toConversation((await write(createChatChannel({ ...at({}), body }))).result, me)
    },
    async updateChannel(conversationId, patch) {
      // An absent `category_id` leaves the category as it is; `null` takes the channel out of its category.
      const body = { name: patch.name, topic: patch.topic, kind: patch.kind, ...(patch.categoryId === undefined ? {} : { category_id: patch.categoryId }) }
      return toConversation((await write(updateChatChannel({ ...at({ conversation_id: conversationId }), body }))).result, me)
    },
    async archiveChannel(conversationId) {
      await write(archiveChatChannel(at({ conversation_id: conversationId })))
    },
    async joinChannel(conversationId) {
      return toConversation((await write(joinChatChannel(at({ conversation_id: conversationId })))).result, me)
    },
    async leaveChannel(conversationId) {
      await write(leaveChatChannel(at({ conversation_id: conversationId })))
    },
    async addMembers(conversationId, userIds) {
      const request = addChatMembers({ ...at({ conversation_id: conversationId }), body: { user_ids: userIds } })
      return toConversation((await write(request)).result, me)
    },
    async removeMember(conversationId, userId) {
      return toConversation((await write(removeChatMember(at({ conversation_id: conversationId, user_id: userId })))).result, me)
    },
    async openDm(userIds) {
      return toConversation((await write(openChatDm({ ...at({}), body: { user_ids: userIds } }))).result, me)
    },

    async createCategory(name) {
      return (await write(createChatCategory({ ...at({}), body: { name } }))).result
    },
    async renameCategory(categoryId, name) {
      return (await write(renameChatCategory({ ...at({ category_id: categoryId }), body: { name } }))).result
    },
    async deleteCategory(categoryId) {
      await write(deleteChatCategory(at({ category_id: categoryId })))
    },
    async move(target, direction) {
      const body = 'categoryId' in target ? { category_id: target.categoryId, direction } : { conversation_id: target.conversationId, direction }
      await write(moveChatItem({ ...at({}), body }))
    },

    uploadAttachment: async () => {
      throw new ChatError('upload_failed', 'Files in chat are not available yet.')
    },
    // Typing needs the live socket.
    sendTyping() {},

    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}
