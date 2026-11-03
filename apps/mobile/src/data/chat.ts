import type { ChatFileRecord, EmojibaseData, FollowedThreadRecord, ThreadPage } from './chat-api'
import type { CategoryRecord, ConversationRecord, ConversationStateRecord, MemberRecord, MessagePage, MessageRecord } from './orbit'

export interface Person {
  id: string
  name: string
}

const isMember = (conversation: ConversationRecord, userId: string) => conversation.member_ids.includes(userId)
const isVisible = (conversation: ConversationRecord, userId: string) => isMember(conversation, userId) && !conversation.archived

/**
 * The count badge of a sidebar row, like the web: every unread message when notify is `all`, else mentions only. `unread`
 * makes the name bold.
 */
export function conversationBadge(
  conversation: ConversationRecord,
  state: ConversationStateRecord | undefined,
  userId: string,
): { count: number; unread: boolean } {
  if (!state || !isVisible(conversation, userId)) return { count: 0, unread: false }
  if (state.notify === 'muted') return { count: state.mention_count, unread: false }
  return { count: state.notify === 'all' ? state.unread_count : state.mention_count, unread: state.unread_count > 0 }
}

/** The Chat tab badge: the sum of the row badges, and `unread` while any row is unread (a dot when the count is 0). */
export function chatBadge(
  conversations: readonly ConversationRecord[],
  states: readonly ConversationStateRecord[],
  userId: string,
): { count: number; unread: boolean } {
  const stateById = new Map(states.map((state) => [state.conversation_id, state]))
  const rows = conversations.map((conversation) => conversationBadge(conversation, stateById.get(conversation.id), userId))
  const count = rows.reduce((total, row) => total + row.count, 0)
  return { count, unread: count > 0 || rows.some((row) => row.unread) }
}

export function conversationTitle(conversation: ConversationRecord, people: readonly Person[], userId: string): string {
  if (conversation.kind !== 'dm') return conversation.name
  const nameOf = (id: string) => people.find((person) => person.id === id)?.name ?? 'Unknown'
  if (conversation.self_dm) return `${nameOf(userId)} (you)`
  const peers = conversation.member_ids.filter((id) => id !== userId)
  return peers.length === 0 ? 'Former member' : peers.map(nameOf).join(', ')
}

export interface ChatSection {
  key: string
  title: string
  conversations: ConversationRecord[]
}

const lastActivity = (conversation: ConversationRecord) => conversation.last_message_at ?? conversation.created_at
const byPosition = (a: ConversationRecord, b: ConversationRecord) => a.position - b.position || a.name.localeCompare(b.name)
const byActivity = (a: ConversationRecord, b: ConversationRecord) => lastActivity(b).localeCompare(lastActivity(a)) || (a.id < b.id ? -1 : 1)

/**
 * The web sidebar: Favorites (when any), each category by position, "Channels" for channels without one, then Direct
 * messages by last activity with the DM to yourself first. Only joined, unarchived conversations; empty sections are left
 * out, as a phone cannot fill them.
 */
export function chatSections(
  conversations: readonly ConversationRecord[],
  categories: readonly CategoryRecord[],
  states: readonly ConversationStateRecord[],
  userId: string,
): ChatSection[] {
  const favoriteIds = new Set(states.filter((state) => state.favorite).map((state) => state.conversation_id))
  const joined = conversations.filter((conversation) => isVisible(conversation, userId))
  const favorites = joined.filter((conversation) => favoriteIds.has(conversation.id))
  const rest = joined.filter((conversation) => !favoriteIds.has(conversation.id))
  const channels = rest.filter((conversation) => conversation.kind !== 'dm')
  const categoryIds = new Set(categories.map((category) => category.id))

  const sections: ChatSection[] = [
    {
      key: 'favorites',
      title: 'Favorites',
      conversations: [
        ...favorites.filter((conversation) => conversation.kind !== 'dm').sort((a, b) => a.name.localeCompare(b.name)),
        ...favorites.filter((conversation) => conversation.kind === 'dm').sort(byActivity),
      ],
    },
    ...[...categories]
      .sort((a, b) => a.position - b.position)
      .map((category) => ({
        key: `category-${category.id}`,
        title: category.name,
        conversations: channels.filter((channel) => channel.category_id === category.id).sort(byPosition),
      })),
    {
      key: 'channels',
      title: 'Channels',
      conversations: channels.filter((channel) => !channel.category_id || !categoryIds.has(channel.category_id)).sort(byPosition),
    },
    {
      key: 'dms',
      title: 'Direct messages',
      conversations: rest
        .filter((conversation) => conversation.kind === 'dm')
        .sort((a, b) => Number(b.self_dm) - Number(a.self_dm) || byActivity(a, b)),
    },
  ]
  return sections.filter((section) => section.conversations.length > 0)
}

const MENTION = /<@([^<>\s]+)>|<#([^<>\s]+)>|<!(channel|here)>/g

/** Stored body to the text shown: `<@u1>` becomes `@Ada Lovelace`, `<#c1>` becomes `#general`. */
export function decodeMentions(body: string, people: readonly Person[], conversations: readonly Person[]): string {
  return body.replace(MENTION, (_token, userId?: string, conversationId?: string, global?: string) => {
    if (global) return `@${global}`
    if (userId) return `@${people.find((person) => person.id === userId)?.name ?? 'unknown'}`
    return `#${conversations.find((conversation) => conversation.id === conversationId)?.name ?? 'unknown'}`
  })
}

/** The main list of a conversation holds root messages and system rows; a thread reply only when also sent to the channel. */
export const inMainList = (message: MessageRecord) => message.thread_root_id === null || message.also_in_channel

export type MessageRow =
  | { type: 'day'; key: string; at: string }
  | { type: 'message'; key: string; message: MessageRecord; groupStart: boolean }

const CONTINUE_WITHIN = 5 * 60_000
const GROUP_SPAN = 10 * 60_000

const groupable = (message: MessageRecord) => message.kind === 'message' && !message.deleted && !message.also_in_channel && !message.forwarded
const localDay = (iso: string) => new Date(iso).toDateString()

/**
 * Messages (oldest first) to rows, like the web: a day separator before the first message of each local day, and for
 * each message whether it starts a group (avatar, name, time) or continues the one above. A reply always starts one.
 */
export function buildMessageRows(messages: readonly MessageRecord[]): MessageRow[] {
  const rows: MessageRow[] = []
  let previous: MessageRecord | null = null
  let groupStartedAt = 0
  for (const message of messages) {
    const at = Date.parse(message.created_at)
    const dayChanged = !previous || localDay(previous.created_at) !== localDay(message.created_at)
    if (dayChanged) rows.push({ type: 'day', key: `day-${message.created_at}`, at: message.created_at })
    const continues =
      previous !== null &&
      !dayChanged &&
      groupable(previous) &&
      groupable(message) &&
      !message.reply_to_id &&
      previous.author_id === message.author_id &&
      at - Date.parse(previous.created_at) < CONTINUE_WITHIN &&
      at - groupStartedAt < GROUP_SPAN
    if (!continues) groupStartedAt = at
    rows.push({ type: 'message', key: message.nonce ?? message.id, message, groupStart: !continues })
    previous = message
  }
  return rows
}

/** The cached pages of one conversation, oldest page first, as TanStack Query keeps an infinite query. */
export interface MessagePages {
  pages: MessagePage[]
  pageParams: unknown[]
}

/**
 * Puts a new or changed message into the cached pages. A changed one replaces its copy wherever it is. A new one is
 * appended to the newest page, but only when that page reaches the present (`after` is null): otherwise it lies beyond
 * what is loaded. Thread replies stay out of the main list.
 */
export function upsertMessage(data: MessagePages, message: MessageRecord): MessagePages {
  if (!inMainList(message)) return data
  const known = data.pages.some((page) => page.items.some((item) => item.id === message.id))
  if (known) {
    return { ...data, pages: data.pages.map((page) => ({ ...page, items: page.items.map((item) => (item.id === message.id ? message : item)) })) }
  }
  const last = data.pages.at(-1)
  if (!last || last.after !== null) return data
  return { ...data, pages: [...data.pages.slice(0, -1), { ...last, items: [...last.items, message] }] }
}

export function removeMessage(data: MessagePages, messageId: string): MessagePages {
  return { ...data, pages: data.pages.map((page) => ({ ...page, items: page.items.filter((item) => item.id !== messageId) })) }
}

/** The list with `state` in place of the old state of its conversation. */
export function upsertState(states: readonly ConversationStateRecord[], state: ConversationStateRecord): ConversationStateRecord[] {
  return states.some((item) => item.conversation_id === state.conversation_id)
    ? states.map((item) => (item.conversation_id === state.conversation_id ? state : item))
    : [...states, state]
}

/** Ties a sent message to its echo, so sending twice (a retry) stores it once. Hermes has no `crypto.randomUUID`. */
export const newNonce = () => `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`

// ---- who may do what

/** Workspace owners and admins manage every channel and the shared categories. */
export const isChatAdmin = (role: MemberRecord['role'] | undefined) => role === 'owner' || role === 'admin'

export interface ConversationPermissions {
  /** Edit name, topic, category and visibility. The creator, or an admin. */
  canManage: boolean
  /** `#general` cannot be archived. */
  canArchive: boolean
  /** Every workspace member is in a public channel: only a private one can be left. */
  canLeave: boolean
  /** Members of a public channel; those who manage a private one. Never `#general` or a DM. */
  canAddPeople: boolean
  /** Those who manage a private channel that is not the default one. */
  canRemovePeople: boolean
}

export function conversationPermissions(conversation: ConversationRecord, userId: string, admin: boolean): ConversationPermissions {
  const channel = conversation.kind !== 'dm'
  const member = isMember(conversation, userId)
  const manager = channel && (admin || conversation.created_by === userId)
  const canManage = manager && !conversation.archived
  const open = channel && !conversation.is_default && !conversation.archived
  return {
    canManage,
    canArchive: canManage && !conversation.is_default,
    canLeave: conversation.kind === 'private' && member,
    canAddPeople: open && (conversation.kind === 'private' ? manager : member),
    canRemovePeople: open && conversation.kind === 'private' && manager,
  }
}

/** What a conversation's `…` sheet offers, in order. The UI holds one label, icon and handler for each. */
export type ConversationAction = 'mark-read' | 'favorite' | 'notify' | 'threads' | 'pins' | 'files' | 'members' | 'edit' | 'leave' | 'archive'

/**
 * The web's `ConversationMenuItems`. The sidebar row has "Mark as read"; the conversation header has Threads, Pinned
 * messages and Files (side panes on a wide screen) and Members instead. Without a state (not a member) only what a
 * visitor can do remains.
 */
export function conversationActions(
  placement: 'row' | 'header',
  state: ConversationStateRecord | undefined,
  permissions: ConversationPermissions,
): ConversationAction[] {
  const actions: ConversationAction[] = []
  if (placement === 'header') actions.push('threads', 'pins', 'files', 'members')
  if (state) {
    if (placement === 'row' && (state.unread_count > 0 || state.mention_count > 0)) actions.push('mark-read')
    actions.push('favorite', 'notify')
  }
  if (permissions.canManage) actions.push('edit')
  if (permissions.canLeave) actions.push('leave')
  if (permissions.canArchive) actions.push('archive')
  return actions
}

/** What the long-press sheet of a message offers, in the web's order. */
export type MessageAction = 'reply' | 'thread' | 'copy' | 'pin' | 'unpin' | 'unread' | 'edit' | 'delete'

/**
 * The web's `actionsFor`: system rows have none; a deleted message keeps "Reply in thread"; edit is for your own text
 * (not a forward, not a sticker alone); delete for your own message or a moderator. Reactions sit above the list.
 */
export function messageActions(message: MessageRecord, userId: string, moderator: boolean, options: { inThread: boolean; canWrite: boolean }): MessageAction[] {
  if (message.kind !== 'message') return []
  const own = message.author_id === userId
  const actions: MessageAction[] = []
  if (options.canWrite && !message.deleted) actions.push('reply')
  if (!options.inThread) actions.push('thread')
  if (message.deleted) return actions
  actions.push('copy', message.pinned ? 'unpin' : 'pin', 'unread')
  const stickerOnly = Boolean(message.sticker_id) && message.body === ''
  if (own && !message.forwarded && !stickerOnly) actions.push('edit')
  if (own || moderator) actions.push('delete')
  return actions
}

// ---- channels and people

export const CHANNEL_NAME_MAX = 80

/** A channel name while it is typed: lowercase, spaces become hyphens. A hyphen at the end stays for the next word. */
export function channelNameInput(value: string): string {
  return value
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^[-#]+/, '')
    .slice(0, CHANNEL_NAME_MAX)
}

/** The name that is sent: no hyphen at the end. Empty when there is nothing to send. */
export const channelNameFinal = (value: string) => channelNameInput(value).replace(/-+$/, '')

/** People a picker offers: not you, not suspended, not in `excludeIds`; by name. */
export function pickablePeople(members: readonly MemberRecord[], userId: string, excludeIds: readonly string[] = []): Person[] {
  return members
    .filter((member) => member.user_id !== userId && !member.suspended_at && !excludeIds.includes(member.user_id))
    .map((member) => ({ id: member.user_id, name: member.display_name }))
    .sort((a, b) => a.name.localeCompare(b.name))
}

/** A new message can go to yourself first, as "Name (you)" (your notes), then `pickablePeople`. */
export function messageablePeople(members: readonly MemberRecord[], userId: string): Person[] {
  const me = members.find((member) => member.user_id === userId)
  const others = pickablePeople(members, userId)
  return me ? [{ id: me.user_id, name: `${me.display_name} (you)` }, ...others] : others
}

export const matchPeople = (people: readonly Person[], text: string) => {
  const query = text.trim().toLowerCase().replace(/^@/, '')
  return query ? people.filter((person) => person.name.toLowerCase().includes(query)) : [...people]
}

// ---- reactions

export function hasReaction(message: MessageRecord, emoji: string, userId: string): boolean {
  return message.reactions.some((reaction) => reaction.emoji === emoji && reaction.user_ids.includes(userId))
}

/** Adds the user's reaction or takes it back; a reaction nobody holds any more is dropped. The optimistic copy of a tap. */
export function toggleReaction(message: MessageRecord, emoji: string, userId: string): MessageRecord {
  const existing = message.reactions.find((reaction) => reaction.emoji === emoji)
  if (!existing) return { ...message, reactions: [...message.reactions, { emoji, user_ids: [userId] }] }
  const userIds = existing.user_ids.includes(userId) ? existing.user_ids.filter((id) => id !== userId) : [...existing.user_ids, userId]
  return {
    ...message,
    reactions: message.reactions.flatMap((reaction) => (reaction !== existing ? [reaction] : userIds.length > 0 ? [{ emoji, user_ids: userIds }] : [])),
  }
}

const DEFAULT_QUICK_REACTIONS = ['👍', '👀', '😂']

/** The quick reactions of the action sheet: the ones the user used most in the loaded messages, then the defaults. */
export function quickReactions(messages: readonly MessageRecord[], userId: string, count = 3): string[] {
  const counts = new Map<string, number>()
  for (const message of messages) {
    for (const reaction of message.reactions) {
      if (reaction.user_ids.includes(userId)) counts.set(reaction.emoji, (counts.get(reaction.emoji) ?? 0) + 1)
    }
  }
  const used = [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([emoji]) => emoji)
  return [...new Set([...used, ...DEFAULT_QUICK_REACTIONS])].slice(0, count)
}

/** `:party_parrot:` → `party_parrot`; anything else is a Unicode emoji. */
export const customEmojiName = (emoji: string) => /^:([a-z0-9_]{2,32}):$/.exec(emoji)?.[1] ?? null

// ---- unreads and threads

export interface UnreadConversation {
  conversation: ConversationRecord
  state: ConversationStateRecord
}

/** The groups of the Unreads view, newest activity first. Muted conversations never show here. */
export function unreadConversations(conversations: readonly ConversationRecord[], states: readonly ConversationStateRecord[], userId: string): UnreadConversation[] {
  const stateById = new Map(states.map((state) => [state.conversation_id, state]))
  return conversations
    .flatMap((conversation) => {
      const state = stateById.get(conversation.id)
      return state && conversationBadge(conversation, state, userId).unread ? [{ conversation, state }] : []
    })
    .sort((a, b) => byActivity(a.conversation, b.conversation))
}

/** The newest unread messages of a group: after the read cursor (ids sort by time), not your own, not system rows. */
export function unreadMessages(messages: readonly MessageRecord[], state: ConversationStateRecord, userId: string, limit = 5): MessageRecord[] {
  const cursor = state.last_read_message_id
  return messages
    .filter((message) => message.kind === 'message' && !message.deleted && message.author_id !== userId && (cursor === null || message.id > cursor))
    .slice(-limit)
}

/** The Threads view: followed threads with unread replies first, then the newest reply first. */
export function sortFollowedThreads(threads: readonly FollowedThreadRecord[]): FollowedThreadRecord[] {
  const lastReplyAt = (thread: FollowedThreadRecord) => thread.last_reply?.created_at ?? thread.root.last_reply_at ?? thread.root.created_at
  return threads
    .filter((thread) => thread.state.following)
    .sort(
      (a, b) =>
        Number(b.state.unread_replies > 0) - Number(a.state.unread_replies > 0) ||
        lastReplyAt(b).localeCompare(lastReplyAt(a)) ||
        (a.root.id < b.root.id ? 1 : -1),
    )
}

/** The Threads row's badge: followed threads with unread replies. */
export const unreadThreadCount = (threads: readonly FollowedThreadRecord[]) => threads.filter((thread) => thread.state.following && thread.state.unread_replies > 0).length

/** The cached pages of a thread, oldest page first. */
export interface ThreadPages {
  pages: ThreadPage[]
  pageParams: unknown[]
}

/**
 * Puts a message into a cached thread: the root itself, or a reply of it. A new reply joins the newest page when that
 * page reaches the present.
 */
export function upsertThreadMessage(data: ThreadPages, message: MessageRecord): ThreadPages {
  const rootId = data.pages[0]?.root.id
  if (message.id === rootId) return { ...data, pages: data.pages.map((page) => ({ ...page, root: message })) }
  if (message.thread_root_id !== rootId) return data
  const known = data.pages.some((page) => page.items.some((item) => item.id === message.id))
  if (known) return { ...data, pages: data.pages.map((page) => ({ ...page, items: page.items.map((item) => (item.id === message.id ? message : item)) })) }
  const last = data.pages.at(-1)
  if (!last || last.after !== null) return data
  return { ...data, pages: [...data.pages.slice(0, -1), { ...last, items: [...last.items, message] }] }
}

// ---- search

export interface TextSegment {
  text: string
  match: boolean
}

/** Cuts the body at the `[start, end)` ranges of a search hit (UTF-16 offsets, as JavaScript counts). Overlaps merge. */
export function splitByRanges(text: string, ranges: readonly (readonly number[])[]): TextSegment[] {
  const sorted = ranges
    .map(([start = 0, end = 0]) => [Math.max(0, start), Math.min(text.length, end)] as const)
    .filter(([start, end]) => end > start)
    .sort((a, b) => a[0] - b[0])
  const segments: TextSegment[] = []
  let at = 0
  for (const [start, end] of sorted) {
    const last = segments.at(-1)
    if (start <= at && last?.match) {
      if (end > at) {
        segments[segments.length - 1] = { text: last.text + text.slice(at, end), match: true }
        at = end
      }
      continue
    }
    const from = Math.max(start, at)
    if (end <= from) continue
    if (from > at) segments.push({ text: text.slice(at, from), match: false })
    segments.push({ text: text.slice(from, end), match: true })
    at = end
  }
  if (at < text.length) segments.push({ text: text.slice(at), match: false })
  return segments
}

/** A long lead before the first match is cut to its last `lead` characters, from a word start, behind `…`. */
export function leadToFirstMatch(segments: readonly TextSegment[], lead = 60): TextSegment[] {
  const [first, ...rest] = segments
  if (!first || first.match || rest.length === 0 || first.text.length <= lead) return [...segments]
  const tail = first.text.slice(-lead)
  const wordStart = tail.search(/\s/)
  return [{ text: `…${wordStart >= 0 ? tail.slice(wordStart + 1) : tail}`, match: false }, ...rest]
}

// ---- files

export const isImage = (file: ChatFileRecord) => file.mime_type.startsWith('image/')

/** `512 B`, `12 KB`, `3.4 MB`. */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

// ---- emoji

export interface EmojiEntry {
  emoji: string
  /** Shortcodes and label words the search matches. */
  names: string[]
}

export interface EmojiGroup {
  key: string
  title: string
  emojis: EmojiEntry[]
}

/** The newest emoji the server's images cover (the web's `EMOJI_VERSION`). */
export const EMOJI_VERSION = 17

/**
 * The web picker's groups from the emojibase files: by group and order, without skin-tone components and without
 * emoji newer than `EMOJI_VERSION`. Search names are the GitHub shortcodes, then the label.
 */
export function emojiGroups(data: EmojibaseData): EmojiGroup[] {
  const titled = (message: string) => message.charAt(0).toUpperCase() + message.slice(1)
  return [...data.groups]
    .filter((group) => group.key !== 'component')
    .sort((a, b) => a.order - b.order)
    .map((group) => ({
      key: group.key,
      title: titled(group.message),
      emojis: data.emojis
        .filter((entry) => entry.group === group.order && entry.version <= EMOJI_VERSION)
        .sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
        .map((entry) => {
          const codes = data.shortcodes[entry.hexcode]
          return { emoji: entry.emoji, names: [...(Array.isArray(codes) ? codes : codes ? [codes] : []), entry.label.toLowerCase()] }
        }),
    }))
    .filter((group) => group.emojis.length > 0)
}

/** Names that start with the query first, then names that contain it; one result per emoji. */
export function searchEmoji(groups: readonly EmojiGroup[], query: string, limit = 64): EmojiEntry[] {
  const text = query.trim().toLowerCase().replace(/^:/, '')
  if (!text) return []
  const all = groups.flatMap((group) => group.emojis)
  const seen = new Set<string>()
  const results: EmojiEntry[] = []
  for (const starts of [true, false]) {
    for (const entry of all) {
      if (results.length >= limit) return results
      if (seen.has(entry.emoji)) continue
      if (entry.names.some((name) => (starts ? name.startsWith(text) : name.includes(text)))) {
        seen.add(entry.emoji)
        results.push(entry)
      }
    }
  }
  return results
}

const BUILT_IN = '😀 😃 😄 😁 😆 😅 😂 🙂 😉 😊 😇 😍 🤩 😘 😋 😜 🤔 🤨 😐 😑 😶 🙄 😏 😬 😌 😴 😷 🤯 😎 🤓 😕 😟 😮 😲 😳 🥺 😢 😭 😱 😤 😡 👍 👎 👏 🙌 🙏 💪 👀 👋 🤝 ✌️ 👌 ❤️ 🧡 💛 💚 💙 💜 🔥 ✨ ⭐ 🎉 🎊 ✅ ❌ ⚠️ 💯 🚀 💡 📌 📎 ☕ 🍕 🎂'

/** The picker's set when the server's emoji data cannot load. */
export const BUILT_IN_EMOJI: EmojiGroup[] = [{ key: 'common', title: 'Common', emojis: BUILT_IN.split(' ').map((emoji) => ({ emoji, names: [] })) }]
