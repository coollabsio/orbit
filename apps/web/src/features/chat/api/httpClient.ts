import { apiClient, CONTRACT_ID, type createApiClient } from '@/api/client'
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
  listChatFiles,
  listChatMessages,
  listChatPins,
  listChatSearch,
  listChatStates,
  listChatThreads,
  listFollowedChatThreads,
  markChatMessageUnread,
  moveChatItem,
  placeChatChannel,
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
import type { ChatEvent as WireEvent, ChatFileRecord } from '@/api/generated/types.gen'
import type { ChatClient } from './client'
import { type LiveSocket, type LiveSocketOptions, openLiveSocket } from './liveSocket'
import { ChatError, type ChatEvent, type ConversationState } from './types'
import {
  toAttachment,
  toChatError,
  toConversation,
  toEvent,
  toFollowedThread,
  toMessage,
  toMessagePage,
  toState,
  toThreadPage,
  toThreadState,
} from './wire'

type ApiClient = ReturnType<typeof createApiClient>

export interface HttpChatClientOptions {
  workspaceId: string
  currentUserId: string
  client?: ApiClient
  /** A "mark as read" for one conversation goes out at most once in this time; default 1000. `0` turns it off. */
  readIntervalMs?: number
  /** The user's inbox changed on the server (a chat mention arrived, or reading a conversation read one). */
  onInboxChanged?: () => void
  /** Opens the live socket; tests put their own in. */
  openSocket?: (options: LiveSocketOptions) => LiveSocket
  /** How long a send that could not reach the server waits for the connection to come back. Default 60000. */
  sendWaitMs?: number
}

/** The pixel size of an image file, if this browser can decode it (not SVG, for one). */
async function imageSize(file: File): Promise<{ width: number; height: number } | null> {
  if (!file.type.startsWith('image/') || typeof createImageBitmap !== 'function') return null
  try {
    const bitmap = await createImageBitmap(file)
    const size = { width: bitmap.width, height: bitmap.height }
    bitmap.close()
    return size
  } catch {
    return null
  }
}

/** A typing signal as the live socket sends it. */
interface WireTyping {
  type: 'typing'
  conversation_id: string
  thread_root_id: string | null
  user_id: string
}

interface WirePresence {
  type: 'presence'
  user_id: string
  online: boolean
}

/**
 * Chat on the Orbit server, for one workspace and the signed-in user. A write's response lists the events that the
 * write caused for the user; they go to the subscribers, so the cache changes the same way for the user's own writes
 * and (through the live socket) for everybody else's. The socket is open while somebody is subscribed.
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

  let socket: LiveSocket | null = null
  /** Who is online, from the socket's `hello` and its presence signals. `null` before the first `hello`. */
  let online: Set<string> | null = null
  let presenceWaiters: ((userIds: string[]) => void)[] = []

  function openSocket() {
    const url = `${(globalThis.location?.origin ?? 'http://localhost').replace(/^http/, 'ws')}/api/v1/workspaces/${workspaceId}/live`
    return (options.openSocket ?? openLiveSocket)({
      url,
      onStatus: (status) => emit({ type: 'connection', status }),
      onResync: () => emit({ type: 'resync' }),
      onHello(userIds) {
        // A reconnect: tell the subscribers who came and went meanwhile.
        const next = new Set(userIds)
        for (const userId of online ?? []) if (!next.has(userId)) emit({ type: 'presence', userId, online: false })
        for (const userId of next) if (online && !online.has(userId)) emit({ type: 'presence', userId, online: true })
        online = next
        for (const resolve of presenceWaiters.splice(0)) resolve(userIds)
      },
      onEvent(topic, event) {
        if (topic === 'presence') {
          const { user_id: userId, online: isOnline } = event as WirePresence
          if (isOnline) online?.add(userId)
          else online?.delete(userId)
          emit({ type: 'presence', userId, online: isOnline })
        } else if (topic === 'inbox') {
          options.onInboxChanged?.()
        } else if (topic === 'chat') {
          const wire = event as WireEvent | WireTyping
          if (wire.type === 'typing') {
            emit({ type: 'typing', conversationId: wire.conversation_id, threadRootId: wire.thread_root_id, userId: wire.user_id })
          } else {
            emit(toEvent(wire, me))
          }
        }
      },
    })
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

  /**
   * A write: its events go to the subscribers before the caller gets the result. The socket sends the same events. If
   * it has delivered them already (`seq`), the response is old news: newer events can be in the cache by now, and the
   * records of the response must not go over them.
   */
  async function write<T extends { events: WireEvent[]; seq: number }>(request: Promise<{ data: T | undefined }>): Promise<T> {
    const data = await call(request)
    if (!socket?.seen(data.seq)) for (const event of data.events) emit(toEvent(event, me))
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
    async listFiles(conversationId) {
      return (await call(listChatFiles(at({ conversation_id: conversationId })))).map(toMessage)
    },
    async searchMessages(input) {
      const query = {
        query: input.query,
        conversation_id: input.conversationId,
        author_id: input.authorId,
        has_file: input.hasFile,
        cursor: input.cursor,
      }
      const page = await call(listChatSearch({ ...at({}), query }))
      return {
        items: page.items.map((hit) => ({
          message: toMessage(hit.message),
          conversationId: hit.conversation_id,
          ranges: hit.ranges.map(([start, end]): [number, number] => [start, end]),
        })),
        cursor: page.cursor,
      }
    },
    /** Known from the socket's first frame. */
    getPresence: () => (online ? Promise.resolve([...online]) : new Promise((resolve) => presenceWaiters.push(resolve))),

    async sendMessage(input) {
      const body = {
        body: input.body,
        thread_root_id: input.threadRootId ?? null,
        also_in_channel: input.alsoInChannel ?? false,
        file_ids: (input.attachments ?? []).map((attachment) => attachment.id),
        nonce: input.nonce,
      }
      const send = () => write(sendChatMessage({ ...at({ conversation_id: input.conversationId }), body }))
      try {
        return toMessage((await send()).result)
      } catch (error) {
        // The request did not reach the server. The message waits for the connection and goes again; the nonce
        // makes that safe even if the first request did arrive.
        if (!(error instanceof ChatError) || !error.unreached || !socket) throw error
        const wait = new Promise<never>((_, reject) => setTimeout(() => reject(error), options.sendWaitMs ?? 60_000))
        // The socket may not know yet that the connection is gone: a new connection is the proof that it is back.
        await Promise.race([socket.reconnected(), wait])
        return toMessage((await send()).result)
      }
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
    async placeCategory(categoryId, beforeId) {
      await write(moveChatItem({ ...at({}), body: { category_id: categoryId, before_id: beforeId } }))
    },
    async placeChannel(conversationId, categoryId, beforeId) {
      await write(placeChatChannel({ ...at({ conversation_id: conversationId }), body: { category_id: categoryId, before_id: beforeId } }))
    },

    /**
     * One request with upload progress, which `fetch` cannot report. The file waits on the server for the message that
     * names it; the size of an image goes along so the list can reserve its space.
     */
    async uploadAttachment(file, { onProgress, signal } = {}) {
      const failed = (message: string) => new ChatError('upload_failed', message)
      if (signal?.aborted) throw failed('The upload was cancelled.')
      const size = await imageSize(file)
      // A cancel while the image was measured: the listener below would never fire.
      if (signal?.aborted) throw failed('The upload was cancelled.')
      return new Promise((resolve, reject) => {
        const request = new XMLHttpRequest()
        const query = size ? `?width=${size.width}&height=${size.height}` : ''
        request.open('POST', `/api/v1/workspaces/${workspaceId}/chat/files${query}`)
        request.setRequestHeader('X-Orbit-Contract', CONTRACT_ID)
        request.withCredentials = true
        request.responseType = 'json'
        request.upload.onprogress = (event) => {
          if (event.lengthComputable) onProgress?.(event.loaded / event.total)
        }
        request.onload = () => {
          if (request.status === 201) resolve(toAttachment(request.response as ChatFileRecord))
          else reject(failed((request.response as { detail?: string } | null)?.detail ?? 'The upload failed.'))
        }
        request.onerror = () => reject(failed('The server could not be reached.'))
        request.onabort = () => reject(failed('The upload was cancelled.'))
        signal?.addEventListener('abort', () => request.abort(), { once: true })
        const body = new FormData()
        body.append('file', file)
        request.send(body)
      })
    },
    sendTyping(conversationId, threadRootId = null) {
      socket?.send({ type: 'typing', conversation_id: conversationId, thread_root_id: threadRootId })
    },

    subscribe(listener) {
      listeners.add(listener)
      socket ??= openSocket()
      return () => {
        listeners.delete(listener)
        if (listeners.size > 0) return
        socket?.close()
        socket = null
        online = null
        presenceWaiters = []
      }
    },
  }
}
