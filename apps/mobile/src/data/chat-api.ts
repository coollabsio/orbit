/**
 * The chat calls the screens make beyond reading the lists (`orbit.ts` has those). A write returns what it made and the
 * live events it caused: the screen applies the events to the cache, as the live socket would, so the change shows at
 * once and in the same way. No React, so a Bun script drives the same functions.
 */
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
  listChatEmoji,
  listChatFiles,
  listChatMessages,
  listChatPins,
  listChatSearch,
  listChatThreads,
  listFollowedChatThreads,
  markChatMessageUnread,
  openChatDm,
  pinChatMessage,
  readAllChat,
  readChatConversation,
  readChatThread,
  removeChatMember,
  removeChatReaction,
  renameChatCategory,
  sendChatMessage,
  updateChatChannel,
  updateChatState,
  uploadChatFile,
} from '@orbit/api/generated/sdk.gen'
import type {
  ChatChannelUpdateBody,
  ChatEvent,
  ChatFileRecord,
  ChatStateBody,
  CustomEmojiRecord,
  FollowedThreadRecord,
  MessagePage,
  SearchPage,
  ThreadPage,
  ThreadStateRecord,
} from '@orbit/api/generated/types.gen'

import type { CategoryRecord, ConversationRecord, ConversationStateRecord, MessageRecord, OrbitClient } from './orbit'

export type { ChatEvent, ChatFileRecord, CustomEmojiRecord, FollowedThreadRecord, SearchPage, ThreadPage, ThreadStateRecord }

/** What a write made, and the live events it caused. */
export interface ChatWrite<T> {
  result: T
  events: ChatEvent[]
}

const at = (client: OrbitClient, workspaceId: string) => ({ client, path: { workspace_id: workspaceId }, throwOnError: true as const })
const inConversation = (client: OrbitClient, workspaceId: string, conversationId: string) => ({
  client,
  path: { workspace_id: workspaceId, conversation_id: conversationId },
  throwOnError: true as const,
})
const onMessage = (client: OrbitClient, workspaceId: string, messageId: string) => ({ client, path: { workspace_id: workspaceId, message_id: messageId }, throwOnError: true as const })
const onThread = (client: OrbitClient, workspaceId: string, rootId: string) => ({ client, path: { workspace_id: workspaceId, root_id: rootId }, throwOnError: true as const })
const write = <T>(data: { result: T; events: ChatEvent[] }): ChatWrite<T> => ({ result: data.result, events: data.events })
const eventsOnly = (data: { events: ChatEvent[] }): ChatWrite<true> => ({ result: true, events: data.events })

// ---- conversations

export interface NewChannel {
  name: string
  topic: string
  kind: 'public' | 'private'
  categoryId: string | null
  /** Private channels only: the people besides the creator. */
  memberIds: string[]
}

export async function createChannel(client: OrbitClient, workspaceId: string, input: NewChannel): Promise<ChatWrite<ConversationRecord>> {
  const body = { name: input.name, topic: input.topic, kind: input.kind, category_id: input.categoryId, member_ids: input.memberIds }
  return write((await createChatChannel({ ...at(client, workspaceId), body })).data)
}

/** Only the fields in `patch` change, so a save does not undo somebody else's change to another field. */
export async function updateChannel(client: OrbitClient, workspaceId: string, conversationId: string, patch: ChatChannelUpdateBody): Promise<ChatWrite<ConversationRecord>> {
  return write((await updateChatChannel({ ...inConversation(client, workspaceId, conversationId), body: patch })).data)
}

export async function archiveChannel(client: OrbitClient, workspaceId: string, conversationId: string): Promise<ChatWrite<ConversationRecord>> {
  return write((await archiveChatChannel(inConversation(client, workspaceId, conversationId))).data)
}

/** A public channel you are not in yet (Browse channels). */
export async function joinChannel(client: OrbitClient, workspaceId: string, conversationId: string): Promise<ChatWrite<ConversationRecord>> {
  return write((await joinChatChannel(inConversation(client, workspaceId, conversationId))).data)
}

export async function leaveChannel(client: OrbitClient, workspaceId: string, conversationId: string): Promise<ChatWrite<true>> {
  return eventsOnly((await leaveChatChannel(inConversation(client, workspaceId, conversationId))).data)
}

export async function addMembers(client: OrbitClient, workspaceId: string, conversationId: string, userIds: string[]): Promise<ChatWrite<ConversationRecord>> {
  return write((await addChatMembers({ ...inConversation(client, workspaceId, conversationId), body: { user_ids: userIds } })).data)
}

export async function removeMember(client: OrbitClient, workspaceId: string, conversationId: string, userId: string): Promise<ChatWrite<ConversationRecord>> {
  return write((await removeChatMember({ client, path: { workspace_id: workspaceId, conversation_id: conversationId, user_id: userId }, throwOnError: true })).data)
}

/** The direct message with exactly these people (the server adds the caller): the existing one or a new one. `[]` is the DM with yourself. */
export async function openDm(client: OrbitClient, workspaceId: string, userIds: string[]): Promise<ChatWrite<ConversationRecord>> {
  return write((await openChatDm({ ...at(client, workspaceId), body: { user_ids: userIds } })).data)
}

export async function setConversationState(client: OrbitClient, workspaceId: string, conversationId: string, body: ChatStateBody): Promise<ChatWrite<ConversationStateRecord>> {
  return write((await updateChatState({ ...inConversation(client, workspaceId, conversationId), body })).data)
}

export async function markRead(client: OrbitClient, workspaceId: string, conversationId: string): Promise<ChatWrite<ConversationStateRecord>> {
  return write((await readChatConversation(inConversation(client, workspaceId, conversationId))).data)
}

export async function markAllRead(client: OrbitClient, workspaceId: string): Promise<ChatWrite<{ states: ConversationStateRecord[]; threads: ThreadStateRecord[] }>> {
  return write((await readAllChat(at(client, workspaceId))).data)
}

// ---- categories

export async function createCategory(client: OrbitClient, workspaceId: string, name: string): Promise<ChatWrite<CategoryRecord>> {
  return write((await createChatCategory({ ...at(client, workspaceId), body: { name } })).data)
}

export async function renameCategory(client: OrbitClient, workspaceId: string, categoryId: string, name: string): Promise<ChatWrite<CategoryRecord>> {
  return write((await renameChatCategory({ client, path: { workspace_id: workspaceId, category_id: categoryId }, body: { name }, throwOnError: true })).data)
}

/** Its channels stay and move to "Channels". */
export async function deleteCategory(client: OrbitClient, workspaceId: string, categoryId: string): Promise<ChatWrite<true>> {
  return eventsOnly((await deleteChatCategory({ client, path: { workspace_id: workspaceId, category_id: categoryId }, throwOnError: true })).data)
}

// ---- messages

export interface OutgoingMessage {
  body: string
  nonce: string
  threadRootId?: string | null
  replyToId?: string | null
  fileIds?: string[]
  /** A thread reply that also shows in the conversation. */
  alsoInChannel?: boolean
}

export async function send(client: OrbitClient, workspaceId: string, conversationId: string, message: OutgoingMessage): Promise<ChatWrite<MessageRecord>> {
  const body = {
    body: message.body,
    nonce: message.nonce,
    thread_root_id: message.threadRootId ?? null,
    reply_to_id: message.replyToId ?? null,
    file_ids: message.fileIds ?? [],
    also_in_channel: message.alsoInChannel ?? false,
  }
  return write((await sendChatMessage({ ...inConversation(client, workspaceId, conversationId), body })).data)
}

/** Where a page of messages starts: around one message (a search result), or before or after a loaded page. */
export type MessageCursor = { around: string } | { before: string } | { after: string }

/** Items are oldest first. */
export async function messagePage(client: OrbitClient, workspaceId: string, conversationId: string, cursor: MessageCursor): Promise<MessagePage> {
  return (await listChatMessages({ ...inConversation(client, workspaceId, conversationId), query: { ...cursor, limit: 50 } })).data
}

export async function editMessage(client: OrbitClient, workspaceId: string, messageId: string, body: string): Promise<ChatWrite<MessageRecord>> {
  return write((await editChatMessage({ ...onMessage(client, workspaceId, messageId), body: { body } })).data)
}

export async function deleteMessage(client: OrbitClient, workspaceId: string, messageId: string): Promise<ChatWrite<true>> {
  return eventsOnly((await deleteChatMessage(onMessage(client, workspaceId, messageId))).data)
}

export async function setPinned(client: OrbitClient, workspaceId: string, messageId: string, pinned: boolean): Promise<ChatWrite<MessageRecord>> {
  return write((await pinChatMessage({ ...onMessage(client, workspaceId, messageId), body: { pinned } })).data)
}

/** Moves the read cursor to just before the message. */
export async function markUnread(client: OrbitClient, workspaceId: string, messageId: string): Promise<ChatWrite<ConversationStateRecord>> {
  return write((await markChatMessageUnread(onMessage(client, workspaceId, messageId))).data)
}

/** `emoji` is the character, or `:name:` for a custom emoji. */
export async function react(client: OrbitClient, workspaceId: string, messageId: string, emoji: string, on: boolean): Promise<ChatWrite<MessageRecord>> {
  const options = { client, path: { workspace_id: workspaceId, message_id: messageId, emoji }, throwOnError: true as const }
  return write((on ? await addChatReaction(options) : await removeChatReaction(options)).data)
}

export async function pins(client: OrbitClient, workspaceId: string, conversationId: string): Promise<MessageRecord[]> {
  return (await listChatPins(inConversation(client, workspaceId, conversationId))).data
}

/** Messages with files, newest first. */
export async function fileMessages(client: OrbitClient, workspaceId: string, conversationId: string): Promise<MessageRecord[]> {
  return (await listChatFiles(inConversation(client, workspaceId, conversationId))).data
}

/** The thread roots of one conversation. */
export async function conversationThreads(client: OrbitClient, workspaceId: string, conversationId: string): Promise<MessageRecord[]> {
  return (await listChatThreads(inConversation(client, workspaceId, conversationId))).data
}

export interface SearchQuery {
  query: string
  conversationId?: string
  authorId?: string
  hasFile?: boolean
  cursor?: string
}

export async function search(client: OrbitClient, workspaceId: string, input: SearchQuery): Promise<SearchPage> {
  const query = { query: input.query || undefined, conversation_id: input.conversationId, author_id: input.authorId, has_file: input.hasFile, cursor: input.cursor }
  return (await listChatSearch({ ...at(client, workspaceId), query })).data
}

// ---- threads

export async function followedThreads(client: OrbitClient, workspaceId: string): Promise<FollowedThreadRecord[]> {
  return (await listFollowedChatThreads(at(client, workspaceId))).data
}

/** Without `before`: the newest replies. Items are oldest first. */
export async function thread(client: OrbitClient, workspaceId: string, rootId: string, before?: string): Promise<ThreadPage> {
  return (await getChatThread({ ...onThread(client, workspaceId, rootId), query: { before, limit: 50 } })).data
}

export async function followThread(client: OrbitClient, workspaceId: string, rootId: string, following: boolean): Promise<ChatWrite<ThreadStateRecord>> {
  return write((await followChatThread({ ...onThread(client, workspaceId, rootId), body: { following } })).data)
}

export async function markThreadRead(client: OrbitClient, workspaceId: string, rootId: string): Promise<ChatWrite<ThreadStateRecord>> {
  return write((await readChatThread(onThread(client, workspaceId, rootId))).data)
}

// ---- files and emoji

/**
 * A file to upload: an expo-file-system `File` in the app (`src/upload-part.ts`), a `Blob` in a browser or Bun.
 * `width` and `height` let the server lay out an image before it loads.
 */
export interface Upload {
  part: Blob
  width?: number
  height?: number
}

/** The file waits on the server for the message that names it in `fileIds`. */
export async function uploadFile(client: OrbitClient, workspaceId: string, upload: Upload): Promise<ChatFileRecord> {
  const { data } = await uploadChatFile({
    ...at(client, workspaceId),
    query: upload.width && upload.height ? { width: upload.width, height: upload.height } : undefined,
    body: { file: upload.part },
    // the generated serializer turns anything but a Blob into JSON; the app's `File` only implements Blob
    bodySerializer: (body) => {
      const form = new FormData()
      form.append('file', (body as { file: Blob }).file)
      return form
    },
  })
  return data
}

export async function customEmoji(client: OrbitClient, workspaceId: string): Promise<CustomEmojiRecord[]> {
  return (await listChatEmoji(at(client, workspaceId))).data
}

/**
 * The emojibase files the web picker loads, from the Orbit server's own copy. The path carries the version the web
 * build bundles; when it moves, the load fails and the picker falls back to its built-in set.
 */
export const EMOJIBASE_PATH = '/assets/emojibase-17.0.0/en'

export interface EmojibaseData {
  emojis: { emoji: string; hexcode: string; label: string; group?: number; order?: number; version: number; tags?: string[] }[]
  groups: { key: string; message: string; order: number }[]
  shortcodes: Record<string, string | string[]>
}

export async function emojibase(server: string): Promise<EmojibaseData> {
  const get = async <T>(path: string): Promise<T> => {
    const response = await fetch(`${server}${EMOJIBASE_PATH}/${path}`)
    if (!response.ok) throw new Error(`Could not load ${path} (${response.status}).`)
    return (await response.json()) as T
  }
  const [emojis, messages, shortcodes] = await Promise.all([
    get<EmojibaseData['emojis']>('data.json'),
    get<{ groups: EmojibaseData['groups'] }>('messages.json'),
    get<EmojibaseData['shortcodes']>('shortcodes/github.json'),
  ])
  return { emojis, groups: messages.groups, shortcodes }
}

/** The address of a server path (a file's or custom emoji's `url`), for an image that sends the bearer token. */
export const serverUrl = (server: string, path: string) => (/^https?:\/\//i.test(path) ? path : `${server.replace(/\/+$/, '')}${path}`)
