import type { Category, Conversation, ConversationState, FollowedThread } from '../api/types'

export interface RowBadge {
  /** Bold name. */
  bold: boolean
  /** The pink count badge; 0 hides it. */
  count: number
  /** The 6px unread square, shown when the row is unread and has no count. */
  marker: boolean
  /** The conversation has a group in the Unreads view. */
  inUnreads: boolean
}

const NO_BADGE: RowBadge = { bold: false, count: 0, marker: false, inUnreads: false }

/**
 * The spec's notification table. The counts themselves come from the client: for a muted conversation `mentionCount`
 * already holds `@user` mentions only.
 */
export function conversationBadge(conversation: Conversation, state: ConversationState | undefined): RowBadge {
  if (!state || !conversation.isMember || conversation.archived) return NO_BADGE
  if (state.notify === 'muted') return { ...NO_BADGE, count: state.mentionCount }
  const unread = state.unreadCount > 0
  const count = state.notify === 'all' ? state.unreadCount : state.mentionCount
  return { bold: unread, count, marker: unread && count === 0, inUnreads: unread }
}

export interface ChatBadges {
  /** The Unreads row: the sum of all count badges. */
  unreads: number
  /** The Threads row: followed threads with unread replies. */
  threads: number
  /** The Chat item in the app sidebar and the mobile dock. */
  total: number
}

export function chatBadges(
  conversations: readonly Conversation[],
  states: readonly ConversationState[],
  followedThreads: readonly FollowedThread[],
): ChatBadges {
  const stateById = new Map(states.map((state) => [state.conversationId, state]))
  const unreads = conversations.reduce(
    (total, conversation) => total + conversationBadge(conversation, stateById.get(conversation.id)).count,
    0,
  )
  const threads = followedThreads.filter((thread) => thread.state.following && thread.state.unreadReplies > 0).length
  return { unreads, threads, total: unreads + threads }
}

export interface UnreadConversation {
  conversation: Conversation
  state: ConversationState
}

/** The groups of the Unreads view, newest activity first. Muted conversations never show here. */
export function unreadConversations(
  conversations: readonly Conversation[],
  states: readonly ConversationState[],
): UnreadConversation[] {
  const stateById = new Map(states.map((state) => [state.conversationId, state]))
  return conversations
    .flatMap((conversation) => {
      const state = stateById.get(conversation.id)
      return state && conversationBadge(conversation, state).inUnreads ? [{ conversation, state }] : []
    })
    .sort((a, b) => lastActivity(b.conversation) - lastActivity(a.conversation))
}

export interface ChatPerson {
  id: string
  name: string
}

/** The members of a DM other than the current user. Empty for a DM with yourself. */
export function dmPeerIds(conversation: Conversation, currentUserId: string): string[] {
  return conversation.memberIds.filter((id) => id !== currentUserId)
}

/** A DM has no name: its title is the other members' names. */
export function dmTitle(conversation: Conversation, people: readonly ChatPerson[], currentUserId: string): string {
  const nameOf = (id: string) => people.find((person) => person.id === id)?.name ?? 'Unknown'
  const peers = dmPeerIds(conversation, currentUserId)
  if (peers.length === 0) return `${nameOf(currentUserId)} (you)`
  return peers.map(nameOf).join(', ')
}

export function conversationTitle(conversation: Conversation, people: readonly ChatPerson[], currentUserId: string): string {
  return conversation.kind === 'dm' ? dmTitle(conversation, people, currentUserId) : conversation.name
}

export interface SidebarSection {
  key: string
  kind: 'favorites' | 'category' | 'channels' | 'dms'
  title: string
  /** Set for `kind: 'category'`. */
  category: Category | null
  conversations: Conversation[]
}

const lastActivity = (conversation: Conversation) => conversation.lastMessageAt ?? conversation.createdAt
const byPosition = (a: Conversation, b: Conversation) => a.position - b.position || a.name.localeCompare(b.name)
const byActivity = (a: Conversation, b: Conversation) => lastActivity(b) - lastActivity(a) || (a.id < b.id ? -1 : 1)

/**
 * The scrolling list of the chat sidebar: Favorites (hidden when empty; a favorite leaves its own group), each shared
 * category by position (kept when empty, so admins can fill it), "Channels" for channels without a category (hidden
 * when empty), then Direct messages by last activity. Only joined, unarchived conversations show.
 */
export function buildSidebarSections(
  conversations: readonly Conversation[],
  categories: readonly Category[],
  states: readonly ConversationState[],
): SidebarSection[] {
  const favoriteIds = new Set(states.filter((state) => state.favorite).map((state) => state.conversationId))
  const joined = conversations.filter((conversation) => conversation.isMember && !conversation.archived)
  const favorites = joined.filter((conversation) => favoriteIds.has(conversation.id))
  const rest = joined.filter((conversation) => !favoriteIds.has(conversation.id))
  const channels = rest.filter((conversation) => conversation.kind !== 'dm')
  const categoryIds = new Set(categories.map((category) => category.id))
  const uncategorized = channels.filter((channel) => !channel.categoryId || !categoryIds.has(channel.categoryId))

  const sections: SidebarSection[] = []
  if (favorites.length > 0) {
    sections.push({
      key: 'favorites',
      kind: 'favorites',
      title: 'Favorites',
      category: null,
      conversations: [
        ...favorites.filter((conversation) => conversation.kind !== 'dm').sort((a, b) => a.name.localeCompare(b.name)),
        ...favorites.filter((conversation) => conversation.kind === 'dm').sort(byActivity),
      ],
    })
  }
  for (const category of [...categories].sort((a, b) => a.position - b.position)) {
    sections.push({
      key: `category-${category.id}`,
      kind: 'category',
      title: category.name,
      category,
      conversations: channels.filter((channel) => channel.categoryId === category.id).sort(byPosition),
    })
  }
  if (uncategorized.length > 0) {
    sections.push({
      key: 'channels',
      kind: 'channels',
      title: 'Channels',
      category: null,
      conversations: uncategorized.sort(byPosition),
    })
  }
  sections.push({
    key: 'dms',
    kind: 'dms',
    title: 'Direct messages',
    category: null,
    conversations: rest.filter((conversation) => conversation.kind === 'dm').sort(byActivity),
  })
  return sections
}

/** The Threads view order: threads with unread replies first, then newest reply first. */
export function sortFollowedThreads(threads: readonly FollowedThread[]): FollowedThread[] {
  const lastReplyAt = (thread: FollowedThread) => thread.lastReply?.createdAt ?? thread.root.lastReplyAt ?? thread.root.createdAt
  return [...threads].sort(
    (a, b) =>
      Number(b.state.unreadReplies > 0) - Number(a.state.unreadReplies > 0) ||
      lastReplyAt(b) - lastReplyAt(a) ||
      (a.root.id < b.root.id ? 1 : -1),
  )
}

/**
 * Where a dragged item goes when it is dropped on a row of `items` (the channels of one section, or the categories,
 * in order): the id of the item that it comes before, or `null` for the end.
 */
export function dropBefore(items: readonly { id: string }[], dragId: string, rowId: string, zone: 'before' | 'after'): string | null {
  const others = items.filter((item) => item.id !== dragId)
  const index = others.findIndex((item) => item.id === rowId) + (zone === 'after' ? 1 : 0)
  return others[index]?.id ?? null
}
