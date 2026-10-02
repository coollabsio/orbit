import type { User } from '@/features/workspaces/models'
import { extractMentions } from '../lib/mentionTokens'
import { hasReaction, toggleReaction } from '../lib/reactions'
import { sortFollowedThreads } from '../lib/sidebar'
import type { ChannelInput, ChatClient, MessageCursor, SendMessageInput } from './client'
import {
  type Attachment,
  type Category,
  ChatError,
  type ChatEvent,
  type Conversation,
  type ConversationKind,
  type ConversationState,
  type FollowedThread,
  MESSAGE_MAX_LENGTH,
  type Message,
  type MessageKind,
  type ReplyPreview,
  type MessagePage,
  type NotifyLevel,
  type SearchHit,
  type ThreadState,
} from './types'

export interface MockChatClientOptions {
  workspaceId: string
  currentUserId: string
  /** The real workspace members. Only ids, roles and the suspended flag are used; the mock invents no people. */
  members: readonly Pick<User, 'id' | 'role' | 'suspended'>[]
  /** Delay of every call; default 150. `0` (tests) also turns simulated activity off. */
  latencyMs?: number
  /** Tests only: simulated replies even with `latencyMs: 0` (they then arrive on the next timer tick). */
  simulateActivity?: boolean
}

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const PAGE_SIZE = 50
const SEARCH_PAGE_SIZE = 20
const REPLY_DELAY = 2500

/** The user's own cursor and settings in one conversation; counts are derived from the messages on demand. */
interface ReadRecord {
  lastReadMessageId: string | null
  notify: NotifyLevel
  favorite: boolean
}

interface FollowRecord {
  conversationId: string
  following: boolean
  lastReadReplyId: string | null
}

interface NewMessage {
  conversationId: string
  authorId: string
  body: string
  threadRootId?: string | null
  kind?: MessageKind
  attachments?: Attachment[]
  alsoInChannel?: boolean
  nonce?: string | null
  createdAt?: number
}

const svgImage = (color: string, label: string) =>
  `data:image/svg+xml,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 400"><rect width="640" height="400" fill="${color}"/><text x="320" y="210" font-family="sans-serif" font-size="32" text-anchor="middle" fill="#fff">${label}</text></svg>`,
  )}`

const GENERAL_LINES = [
  'Morning. The release branch is cut, changelog is in the doc.',
  'I will take the import bug today.',
  'Does anyone know why the nightly backup ran twice?',
  'It ran twice because the schedule was saved in two time zones. Fixed now.',
  'Thanks, that explains the duplicate file.',
  'Standup notes are posted.',
  'The staging server is back. It was a full disk.',
  'Can someone review the settings page copy before lunch?',
  'Reviewed. Two small wording changes, both in the first section.',
  'Merged.',
  'Reminder: planning is on Thursday, not Wednesday.',
  'I moved my tasks for this cycle into the board.',
  'The new keyboard shortcuts feel good. `Alt+Up` is my favorite.',
  'Customer call went well. They asked for CSV export of task lists.',
  'CSV export is small. I can write it up as a task.',
  'Please do.',
  'Lunch?',
  'In ten minutes.',
  'The docs editor now keeps the cursor when someone else types. Try it.',
  'Tried it with three people in one page. No jumps.',
  'Good work on that one.',
  'I am out tomorrow morning, back after 13:00.',
  'Noted.',
  'Last call for items for the release notes.',
]

const GENERAL_GAPS = [1, 2, 1, 8, 1, 1, 14, 3, 1, 25, 2, 1, 1, 6, 95]
const GENERAL_AUTHORS = [0, 0, 0, 1, 1, 2, 0, 1, 1, 1, 3, 2, 2, 0, 1]

const LONG_MESSAGE = [
  'Notes from the release review, so they are in one place:',
  '',
  ...Array.from({ length: 24 }, (_, index) => `${index + 1}. Checked item ${index + 1} of the release list and wrote the result in the doc.`),
  '',
  'Nothing blocks the release. The two open items are cosmetic and have tasks.',
].join('\n')

const SIMULATED_REPLIES = [
  'Got it, thanks.',
  'Makes sense. I will look at it after lunch.',
  'Agreed.',
  'Can you put that in a task so it does not get lost?',
  'Good point. Let me check and get back to you.',
  'Done on my side.',
]

/**
 * The whole chat backend in memory, for one workspace and one signed-in user. It owns unread and mention counting the
 * way the server will, and reports every change (the user's own too) through `subscribe`. State resets on reload.
 */
function replyPreview(reply: Message | undefined): ReplyPreview | null {
  return reply ? { authorId: reply.authorId, body: reply.body, createdAt: reply.createdAt } : null
}

export function createMockChatClient(options: MockChatClientOptions): ChatClient {
  const me = options.currentUserId
  const latencyMs = options.latencyMs ?? 150
  const others = options.members.filter((member) => member.id !== me && !member.suspended).map((member) => member.id)
  const simulate = others.length > 0 && (options.simulateActivity ?? latencyMs !== 0)
  const isAdmin = ['Owner', 'Admin'].includes(options.members.find((member) => member.id === me)?.role ?? '')

  const conversations = new Map<string, Conversation>()
  let categories: Category[] = []
  /** Stored messages are never mutated: a change puts a new object, so cached copies stay as they were. */
  const messages = new Map<string, Message>()
  const timelines = new Map<string, Message[]>()
  const reads = new Map<string, ReadRecord>()
  const follows = new Map<string, FollowRecord>()
  const seeded = new Set<string>()
  const pendingReplies = new Set<string>()
  const listeners = new Set<(event: ChatEvent) => void>()
  let counter = 0
  let simulatedCount = 0

  /** Zero-padded, so ids sort by creation order as strings. */
  const nextId = (prefix: string) => `${prefix}${String(++counter).padStart(8, '0')}`
  const wait = () => (latencyMs > 0 ? new Promise<void>((resolve) => setTimeout(resolve, latencyMs)) : Promise.resolve())
  const emit = (event: ChatEvent) => {
    for (const listener of [...listeners]) listener(event)
  }

  const timeline = (conversationId: string) => timelines.get(conversationId) ?? []
  const mainList = (conversationId: string) =>
    timeline(conversationId).filter((message) => message.threadRootId === null || message.alsoInChannel)
  const repliesOf = (root: Message) => timeline(root.conversationId).filter((message) => message.threadRootId === root.id)
  const isMember = (conversation: Conversation) => conversation.memberIds.includes(me)
  const readable = (conversation: Conversation) => isMember(conversation) || conversation.kind === 'public'
  const view = (conversation: Conversation): Conversation => ({
    ...conversation,
    memberIds: [...conversation.memberIds],
    isMember: isMember(conversation),
  })
  const canManage = (conversation: Conversation) => conversation.kind !== 'dm' && (isAdmin || conversation.createdBy === me)

  /** A private channel the user is not in does not exist for them. */
  function conversationOf(conversationId: string): Conversation {
    const conversation = conversations.get(conversationId)
    if (!conversation || !readable(conversation)) throw new ChatError('not_found', 'This conversation is not available.')
    return conversation
  }

  function messageOf(messageId: string): Message {
    const message = messages.get(messageId)
    if (!message) throw new ChatError('not_found', 'This message no longer exists.')
    conversationOf(message.conversationId)
    return message
  }

  function memberConversationOf(conversationId: string): Conversation {
    const conversation = conversationOf(conversationId)
    if (!isMember(conversation)) throw new ChatError('forbidden', 'Join the channel first.')
    return conversation
  }

  function putConversation(conversation: Conversation) {
    conversations.set(conversation.id, conversation)
    emit({ type: 'conversation.changed', conversation: view(conversation) })
  }

  function putMessage(message: Message) {
    messages.set(message.id, message)
    const list = timeline(message.conversationId)
    const index = list.findIndex((item) => item.id === message.id)
    if (index >= 0) list[index] = message
  }

  function removeMessage(message: Message) {
    messages.delete(message.id)
    timelines.set(
      message.conversationId,
      timeline(message.conversationId).filter((item) => item.id !== message.id),
    )
    follows.delete(message.id)
    emit({
      type: 'message.deleted',
      conversationId: message.conversationId,
      messageId: message.id,
      threadRootId: message.threadRootId,
    })
  }

  /** The spec's counting rules for the main list. Thread replies count here only when also sent to the channel. */
  function stateOf(conversationId: string): ConversationState | null {
    const read = reads.get(conversationId)
    if (!read) return null
    const unread = mainList(conversationId).filter(
      (message) =>
        message.kind === 'message' &&
        !message.deleted &&
        message.authorId !== me &&
        (read.lastReadMessageId === null || message.id > read.lastReadMessageId),
    )
    // `@channel` and `@here` are mentions, except in a muted conversation.
    const mentions = unread.filter(
      (message) => message.mentions.userIds.includes(me) || (read.notify !== 'muted' && (message.mentions.channel || message.mentions.here)),
    )
    return { conversationId, ...read, unreadCount: unread.length, mentionCount: mentions.length }
  }

  function threadStateOf(rootId: string): ThreadState | null {
    const follow = follows.get(rootId)
    const root = messages.get(rootId)
    if (!follow || !root) return null
    const unread = follow.following
      ? repliesOf(root).filter(
          (reply) => !reply.deleted && reply.authorId !== me && (follow.lastReadReplyId === null || reply.id > follow.lastReadReplyId),
        )
      : []
    return {
      rootId,
      ...follow,
      unreadReplies: unread.length,
      mentionCount: unread.filter((reply) => reply.mentions.userIds.includes(me)).length,
    }
  }

  function emitState(conversationId: string): ConversationState {
    const state = stateOf(conversationId)
    if (!state) throw new ChatError('not_found', 'This conversation is not available.')
    emit({ type: 'state.changed', state })
    return state
  }

  function emitThreadState(rootId: string): ThreadState {
    const state = threadStateOf(rootId)
    if (!state) throw new ChatError('not_found', 'This thread no longer exists.')
    emit({ type: 'thread.changed', state })
    return state
  }

  /** Follow rules: the user replied; or (unless already following) was mentioned, started the thread, or it is a 1:1 DM. */
  function followAfterReply(conversation: Conversation, root: Message, reply: Message, previousReplyId: string | null) {
    const existing = follows.get(root.id)
    if (reply.authorId === me) {
      follows.set(root.id, { conversationId: root.conversationId, following: true, lastReadReplyId: reply.id })
      return
    }
    if (existing?.following) return
    const mentioned = reply.mentions.userIds.includes(me)
    const automatic =
      !existing &&
      (root.authorId === me ||
        root.mentions.userIds.includes(me) ||
        (conversation.kind === 'dm' && conversation.memberIds.length <= 2 && isMember(conversation)))
    if (mentioned || automatic) {
      follows.set(root.id, { conversationId: root.conversationId, following: true, lastReadReplyId: previousReplyId })
    }
  }

  /** Every message enters here: the user's, simulated replies, system rows and the seed. */
  function addMessage(input: NewMessage): Message {
    const conversation = conversations.get(input.conversationId)!
    const root = input.threadRootId ? (messages.get(input.threadRootId) ?? null) : null
    const mentions = extractMentions(input.body)
    // `@channel` and `@here` do nothing in a thread.
    if (root) Object.assign(mentions, { channel: false, here: false })
    const message: Message = {
      id: nextId('m'),
      conversationId: conversation.id,
      threadRootId: root?.id ?? null,
      kind: input.kind ?? 'message',
      authorId: input.authorId,
      body: input.body,
      mentions,
      createdAt: input.createdAt ?? Date.now(),
      editedAt: null,
      deleted: false,
      attachments: input.attachments ?? [],
      reactions: [],
      pinned: false,
      alsoInChannel: Boolean(root && input.alsoInChannel),
      nonce: input.nonce ?? null,
      replyCount: 0,
      lastReplyAt: null,
      replyUserIds: [],
      lastReply: null,
    }
    const inMainList = !root || message.alsoInChannel
    const previousReplyId = root ? (repliesOf(root).at(-1)?.id ?? null) : null
    messages.set(message.id, message)
    timelines.set(conversation.id, [...timeline(conversation.id), message])
    if (inMainList) conversations.set(conversation.id, { ...conversation, lastMessageAt: message.createdAt })
    emit({ type: 'message.created', message })

    if (root) {
      const nextRoot: Message = {
        ...root,
        replyCount: root.replyCount + 1,
        lastReplyAt: message.createdAt,
        replyUserIds: root.replyUserIds.includes(message.authorId) ? root.replyUserIds : [...root.replyUserIds, message.authorId],
        lastReply: replyPreview(message),
      }
      putMessage(nextRoot)
      emit({ type: 'message.updated', message: nextRoot })
      followAfterReply(conversation, root, message, previousReplyId)
      if (follows.has(root.id)) emitThreadState(root.id)
    }
    const read = reads.get(conversation.id)
    if (read && inMainList) {
      // Sending a message reads the conversation up to it.
      if (message.authorId === me) reads.set(conversation.id, { ...read, lastReadMessageId: message.id })
      emitState(conversation.id)
    }
    return message
  }

  function addConversation(input: {
    kind: ConversationKind
    name?: string
    topic?: string
    categoryId?: string | null
    memberIds: string[]
    createdBy: string
    isDefault?: boolean
    createdAt?: number
  }): Conversation {
    const categoryId = input.categoryId ?? null
    const siblings = [...conversations.values()].filter((item) => item.kind !== 'dm' && item.categoryId === categoryId)
    const conversation: Conversation = {
      id: nextId('c'),
      kind: input.kind,
      name: input.name ?? '',
      topic: input.topic ?? '',
      categoryId,
      position: input.kind === 'dm' ? 0 : Math.max(-1, ...siblings.map((item) => item.position)) + 1,
      memberIds: [...new Set(input.memberIds)],
      isMember: false,
      isDefault: input.isDefault ?? false,
      archived: false,
      createdBy: input.createdBy,
      createdAt: input.createdAt ?? Date.now(),
      lastMessageAt: null,
    }
    conversations.set(conversation.id, conversation)
    timelines.set(conversation.id, [])
    if (isMember(conversation)) startReading(conversation)
    return conversation
  }

  /** Channels start at "Mentions only", DMs at "All messages"; a joined conversation starts fully read. */
  function startReading(conversation: Conversation) {
    reads.set(conversation.id, {
      lastReadMessageId: mainList(conversation.id).at(-1)?.id ?? null,
      notify: conversation.kind === 'dm' ? 'all' : 'mentions',
      favorite: false,
    })
  }

  function page(list: Message[], cursor: MessageCursor = {}): MessagePage {
    const limit = cursor.limit ?? PAGE_SIZE
    let start = Math.max(0, list.length - limit)
    if (cursor.around !== undefined) {
      const index = list.findIndex((message) => message.id >= cursor.around!)
      start = Math.max(0, (index < 0 ? list.length : index) - Math.floor(limit / 2))
    } else if (cursor.before !== undefined) {
      const end = list.findIndex((message) => message.id >= cursor.before!)
      list = list.slice(0, end < 0 ? list.length : end)
      start = Math.max(0, list.length - limit)
    } else if (cursor.after !== undefined) {
      const first = list.findIndex((message) => message.id > cursor.after!)
      start = first < 0 ? list.length : first
    }
    const items = list.slice(start, start + limit)
    const hasNewer = cursor.before !== undefined ? items.length > 0 : start + limit < list.length
    return {
      items,
      before: items.length > 0 && start > 0 ? items[0].id : null,
      after: items.length > 0 && hasNewer ? items.at(-1)!.id : null,
    }
  }

  function normalizeName(name: string, exceptId?: string): string {
    const normalized = name.trim().toLowerCase().replace(/\s+/g, '-').slice(0, 80)
    if (!normalized) throw new ChatError('conflict', 'A channel needs a name.')
    const taken = [...conversations.values()].some(
      (item) => item.kind !== 'dm' && !item.archived && item.id !== exceptId && item.name === normalized,
    )
    if (taken) throw new ChatError('conflict', `A channel named #${normalized} already exists.`)
    return normalized
  }

  function checkBody(body: string, attachments: Attachment[] = []) {
    if (body.length > MESSAGE_MAX_LENGTH) {
      throw new ChatError('too_long', `A message can have at most ${MESSAGE_MAX_LENGTH} characters.`)
    }
    if (body.trim() === '' && attachments.length === 0) throw new ChatError('conflict', 'A message needs text or a file.')
  }

  function requireAdmin(action: string) {
    if (!isAdmin) throw new ChatError('forbidden', `Only workspace owners and admins can ${action}.`)
  }

  function requireManage(conversation: Conversation, action: string) {
    if (!canManage(conversation)) {
      throw new ChatError('forbidden', `Only the channel's creator and workspace owners and admins can ${action}.`)
    }
  }

  /** Another member "types" and answers, so the live path (typing, unread counts, auto-follow) is used. */
  function simulateReply(sent: Message) {
    const conversation = conversations.get(sent.conversationId)
    if (!simulate || !conversation || (conversation.kind !== 'dm' && !seeded.has(conversation.id))) return
    const responders = conversation.memberIds.filter((id) => id !== me)
    const key = `${sent.conversationId}:${sent.threadRootId ?? ''}`
    if (responders.length === 0 || pendingReplies.has(key)) return
    pendingReplies.add(key)
    const turn = simulatedCount++
    const userId = responders[turn % responders.length]
    const delay = latencyMs === 0 ? 0 : REPLY_DELAY
    setTimeout(
      () => emit({ type: 'typing', conversationId: sent.conversationId, threadRootId: sent.threadRootId, userId }),
      delay * 0.3,
    )
    setTimeout(() => {
      pendingReplies.delete(key)
      const current = conversations.get(sent.conversationId)
      if (!current || current.archived || !current.memberIds.includes(userId)) return
      if (sent.threadRootId && !messages.has(sent.threadRootId)) return
      addMessage({
        conversationId: current.id,
        authorId: userId,
        threadRootId: sent.threadRootId,
        body: SIMULATED_REPLIES[turn % SIMULATED_REPLIES.length],
      })
    }, delay)
  }

  function seed() {
    const now = Date.now()
    /** 0 is the current user; 1, 2, … are the other members in turn. Alone in the workspace, everything is the user's. */
    const who = (index: number) => (index === 0 || others.length === 0 ? me : others[(index - 1) % others.length])
    const everyone = [me, ...others]
    const created = now - 30 * DAY
    const origin = globalThis.location?.origin ?? ''
    const post = (conversation: Conversation, minutesAgo: number, author: number, body: string, extra: Partial<NewMessage> = {}) =>
      addMessage({ conversationId: conversation.id, authorId: who(author), body, createdAt: now - minutesAgo * MINUTE, ...extra })
    const react = (message: Message, emoji: string, authors: number[]) => {
      const current = messages.get(message.id)!
      putMessage({ ...current, reactions: [...current.reactions, { emoji, userIds: [...new Set(authors.map(who))] }] })
    }
    /** Leaves the last `count` main-list messages unread (they count only when another member wrote them). */
    const leaveUnread = (conversation: Conversation, count: number) => {
      const list = mainList(conversation.id)
      const read = reads.get(conversation.id)
      if (read) reads.set(conversation.id, { ...read, lastReadMessageId: list.at(-1 - count)?.id ?? null })
    }
    const channel = (
      name: string,
      topic: string,
      categoryId: string | null,
      memberIds: string[],
      extra: { kind?: ConversationKind; createdBy?: number } = {},
    ) => {
      const conversation = addConversation({
        kind: extra.kind ?? 'public',
        name,
        topic,
        categoryId,
        memberIds,
        createdBy: who(extra.createdBy ?? 1),
        createdAt: created,
      })
      seeded.add(conversation.id)
      return conversation
    }

    const product: Category = { id: nextId('g'), name: 'Product', position: 0 }
    const company: Category = { id: nextId('g'), name: 'Company', position: 1 }
    categories = [product, company]

    const general = addConversation({
      kind: 'public',
      name: 'general',
      topic: 'Everything that concerns the whole team',
      memberIds: everyone,
      createdBy: who(1),
      isDefault: true,
      createdAt: created,
    })
    seeded.add(general.id)

    // 150 messages over five days. Minute gaps and author runs repeat, so every grouping rule shows up.
    const offsets: number[] = []
    let offset = 3
    for (let fromEnd = 0; fromEnd < 150; fromEnd++) {
      offsets.unshift(offset)
      offset += GENERAL_GAPS[fromEnd % GENERAL_GAPS.length] + (fromEnd % 30 === 29 ? 20 * 60 : 0)
    }
    const special: Record<number, { author?: number; body?: string; attachments?: Attachment[] }> = {
      96: { author: 2, body: LONG_MESSAGE },
      104: {
        author: 1,
        body: `The fix for the import is in ${origin}/tasks/0193a5b2-7c41-7e55-9a10-3f2b6d8e4c01\n\n\`\`\`ts\nfunction chunk<T>(items: T[], size: number): T[][] {\n  return Array.from({ length: Math.ceil(items.length / size) }, (_, index) => items.slice(index * size, index * size + size))\n}\n\`\`\`\n\nIt imports in chunks of 200 now.`,
      },
      110: {
        author: 2,
        body: 'Two options for the empty state:',
        attachments: [
          { id: nextId('a'), fileName: 'empty-state-a.svg', mimeType: 'image/svg+xml', fileSize: 412, url: svgImage('#f2458f', 'Option A'), width: 640, height: 400 },
          { id: nextId('a'), fileName: 'empty-state-b.svg', mimeType: 'image/svg+xml', fileSize: 409, url: svgImage('#742f4d', 'Option B'), width: 640, height: 400 },
          { id: nextId('a'), fileName: 'logo.svg', mimeType: 'image/svg+xml', fileSize: 1024, url: '/logo.svg', width: 220, height: 220 },
        ],
      },
      114: {
        author: 0,
        body: 'Release notes draft is attached.',
        attachments: [
          {
            id: nextId('a'),
            fileName: 'release-notes.txt',
            mimeType: 'text/plain',
            fileSize: 64,
            url: `data:text/plain,${encodeURIComponent('Orbit release notes\n\n- Chat\n- Keyboard shortcuts\n- Faster import\n')}`,
          },
        ],
      },
      118: { author: 0, body: 'Should the import run in the background, or keep the dialog open until it ends?' },
      124: { author: 1, body: 'We moved the weekly review to **Thursday 10:00**. Add your items to the doc before then.' },
      130: { author: 2, body: 'Who can pair on the sidebar resize bug this week?' },
      136: { author: 3, body: 'I wrote down how the backup restore works, in case someone needs it at night.' },
      147: { author: 1, body: 'The build is green again.' },
      148: { author: 1, body: 'I also removed the flaky test and opened a task to rewrite it.' },
      149: { author: 2, body: 'Thanks. I will pick up that task.' },
    }
    const generalMessages = offsets.map((minutesAgo, index) => {
      const item = special[index] ?? {}
      const message = post(
        general,
        minutesAgo,
        item.author ?? GENERAL_AUTHORS[index % GENERAL_AUTHORS.length],
        item.body ?? GENERAL_LINES[index % GENERAL_LINES.length],
        { attachments: item.attachments },
      )
      if (index === 124) {
        putMessage({ ...message, pinned: true })
        addMessage({ conversationId: general.id, authorId: who(2), kind: 'pin', body: '', createdAt: message.createdAt + 20_000 })
      }
      return message
    })
    const reply = (root: Message, minutesLater: number, author: number, body: string) =>
      addMessage({
        conversationId: root.conversationId,
        threadRootId: root.id,
        authorId: who(author),
        body,
        createdAt: root.createdAt + minutesLater * MINUTE,
      })

    // A thread the user started (followed); its last two replies are unread.
    const started = generalMessages[118]
    const firstReply = reply(started, 2, 1, 'Background. A dialog that stays open for minutes gets closed by accident.')
    reply(started, 5, 2, 'Background, and a toast with a link when it ends.')
    reply(started, 9, 1, 'I can add the toast.')
    const startedFollow = follows.get(started.id)
    if (startedFollow) follows.set(started.id, { ...startedFollow, lastReadReplyId: firstReply.id })

    // A thread by other members where a reply mentions the user: followed from that reply on, with a mention.
    const pairing = generalMessages[130]
    reply(pairing, 3, 1, 'I have time on Wednesday.')
    reply(pairing, 6, 2, `<@${me}> you wrote the resize code, can you join for the first half hour?`)

    // A thread between other members: not followed.
    const restore = generalMessages[136]
    reply(restore, 4, 1, 'Thank you. I linked it from the runbook.')
    reply(restore, 8, 2, 'Read it. One question: does restore keep the API tokens?')

    react(generalMessages[124], '👍', [0, 2, 3])
    react(generalMessages[110], '🎨', [1])
    react(generalMessages[110], '👀', [0, 1])
    react(generalMessages[147], '🎉', [2, 3])
    react(generalMessages[104], '🚀', [0])
    leaveUnread(general, 3)

    const announcements = channel('announcements', 'Company news. Replies go in threads.', company.id, everyone)
    post(announcements, 3 * 24 * 60, 1, 'Welcome to the new chat. Channels are grouped by category; star the ones you use most.')
    post(announcements, 26 * 60, 1, 'The office is closed on Friday.')
    const favorite = reads.get(announcements.id)
    if (favorite) reads.set(announcements.id, { ...favorite, favorite: true })

    const random = channel('random', 'Not work', company.id, everyone, { createdBy: 2 })
    post(random, 9 * 60, 2, 'Found a good coffee place next to the station.')
    post(random, 8 * 60 + 50, 3, 'Name?')
    post(random, 8 * 60 + 48, 2, 'Kaffeewerk. Closed on Mondays.')
    post(random, 50, 1, '<!channel> cake in the kitchen.')
    post(random, 45, 3, 'On my way.')
    const muted = reads.get(random.id)
    if (muted) reads.set(random.id, { ...muted, notify: 'muted' })
    leaveUnread(random, 2)

    const engineering = channel('engineering', 'Code, reviews and incidents', product.id, everyone)
    post(engineering, 28 * 60, 0, 'The migration for task labels is ready for review.')
    post(engineering, 27 * 60 + 40, 1, 'Looking at it now.')
    post(engineering, 27 * 60 + 12, 1, 'Approved. One comment about the index name.')
    post(engineering, 27 * 60, 0, 'Renamed. Merging.')
    post(engineering, 95, 2, 'Deploy to staging is done.')
    post(engineering, 40, 1, `<@${me}> can you check the import on staging? It uses your parser.`)
    post(engineering, 38, 1, 'No hurry, today is fine.')
    leaveUnread(engineering, 3)

    const design = channel('design', 'Screens, flows and words', product.id, everyone, { createdBy: 2 })
    addMessage({ conversationId: design.id, authorId: who(3), kind: 'join', body: '', createdAt: now - 2 * DAY })
    post(design, 2 * 24 * 60 - 5, 2, 'New icons for the sidebar are in the shared folder.')
    post(design, 2 * 24 * 60 - 9, 0, 'They look sharp at 20px. Good.')
    post(design, 70, 2, 'I changed the unread marker to a square, like the logo pixel.')
    post(design, 66, 3, 'I like it. It reads well in both themes.')
    leaveUnread(design, 2)

    const support = channel('support', 'Customer questions', product.id, everyone)
    post(support, 5 * 60, 3, 'A customer asks if they can import from a second Notion workspace.')
    post(support, 4 * 60 + 52, 0, 'Yes. Each import is separate; the pages land in the teamspace they pick.')
    post(support, 4 * 60 + 50, 3, 'Thanks, I will answer them.')
    leaveUnread(support, 0)

    // Public and not joined: shows in Browse and can be read.
    const marketing = channel('marketing', 'Website, launch posts and video', company.id, others)
    post(marketing, 6 * 60, 1, 'The launch video draft is ready. Feedback until Wednesday please.')
    post(marketing, 5 * 60 + 30, 2, 'The first ten seconds are great. The end is a bit long.')

    const leadership = channel('leadership', 'Planning and hiring decisions', null, [me, ...others.slice(0, 2)], { kind: 'private' })
    post(leadership, 30 * 60, 1, 'Budget for the next quarter is in the doc. Comments by Friday.')
    post(leadership, 29 * 60, 0, 'Read it. I added two comments on the hosting line.')
    leaveUnread(leadership, 0)

    // Private and not joined: the user must never see it. It needs other members to exist at all.
    if (others.length > 0) {
      const hiring = channel('hiring', 'Candidates', null, others, { kind: 'private' })
      post(hiring, 12 * 60, 1, 'Two interviews are planned for next week.')
    }

    const directLines = [
      ['Do you have ten minutes for the import question?', 'Yes, call me after the standup.', 'Calling now.', 'Sent you the notes from the call.'],
      ['Can you look at my pull request when you have time?', 'Done. Two comments, both small.', 'Fixed both, thanks.'],
      ['Are you at the office tomorrow?', 'Yes, from ten.'],
    ]
    others.slice(0, 3).forEach((userId, index) => {
      const dm = addConversation({ kind: 'dm', memberIds: [me, userId], createdBy: userId, createdAt: created })
      seeded.add(dm.id)
      const lines = directLines[index]
      lines.forEach((line, lineIndex) => {
        const minutesAgo = (index * 7 + 1) * 60 - lineIndex * 3
        // The other member starts, then the two take turns; the first DM ends with two messages from them.
        const author = lineIndex % 2 === 0 || (index === 0 && lineIndex === lines.length - 1) ? index + 1 : 0
        post(dm, minutesAgo, author, line)
      })
      leaveUnread(dm, index === 0 ? 2 : 0)
    })
    if (others.length >= 2) {
      const group = addConversation({ kind: 'dm', memberIds: [me, others[0], others[1]], createdBy: me, createdAt: created })
      seeded.add(group.id)
      post(group, 20 * 60, 0, 'Shall we do the release together on Thursday?')
      post(group, 19 * 60 + 50, 1, 'Yes. I will prepare the checklist.')
      post(group, 19 * 60 + 45, 2, 'I am in.')
      leaveUnread(group, 0)
    }
  }

  seed()

  return {
    async listConversations() {
      await wait()
      return [...conversations.values()].filter((conversation) => !conversation.archived && readable(conversation)).map(view)
    },

    async listCategories() {
      await wait()
      return [...categories].sort((a, b) => a.position - b.position)
    },

    async listStates() {
      await wait()
      return [...reads.keys()].flatMap((conversationId) => {
        const state = conversations.get(conversationId)?.archived ? null : stateOf(conversationId)
        return state ? [state] : []
      })
    },

    async listMessages(conversationId, cursor) {
      await wait()
      conversationOf(conversationId)
      return page(mainList(conversationId), cursor)
    },

    async getThread(rootId, cursor) {
      await wait()
      const root = messageOf(rootId)
      if (root.threadRootId !== null) throw new ChatError('not_found', 'This thread no longer exists.')
      return { ...page(repliesOf(root), cursor), root, state: threadStateOf(rootId) }
    },

    async listThreads(conversationId) {
      await wait()
      conversationOf(conversationId)
      return timeline(conversationId)
        .filter((message) => message.threadRootId === null && message.replyCount > 0)
        .sort((a, b) => (b.lastReplyAt ?? 0) - (a.lastReplyAt ?? 0))
    },

    async listFollowedThreads() {
      await wait()
      const threads = [...follows.keys()].flatMap((rootId): FollowedThread[] => {
        const state = threadStateOf(rootId)
        const root = messages.get(rootId)
        const conversation = root && conversations.get(root.conversationId)
        if (!state?.following || !root || !conversation || !readable(conversation)) return []
        return [{ root, conversationId: root.conversationId, state, lastReply: repliesOf(root).at(-1) ?? null }]
      })
      return sortFollowedThreads(threads)
    },

    async listPins(conversationId) {
      await wait()
      conversationOf(conversationId)
      return timeline(conversationId)
        .filter((message) => message.pinned)
        .reverse()
    },

    async listFiles(conversationId) {
      await wait()
      conversationOf(conversationId)
      return timeline(conversationId)
        .filter((message) => message.attachments.length > 0)
        .reverse()
    },

    async searchMessages(input) {
      await wait()
      const query = input.query.trim().toLowerCase()
      const hits = [...messages.values()]
        .filter((message) => {
          const conversation = conversations.get(message.conversationId)
          if (!conversation || !readable(conversation) || message.kind !== 'message' || message.deleted) return false
          if (input.conversationId && message.conversationId !== input.conversationId) return false
          if (input.authorId && message.authorId !== input.authorId) return false
          if (input.hasFile && message.attachments.length === 0) return false
          if (input.cursor && message.id >= input.cursor) return false
          return query === '' || message.body.toLowerCase().includes(query)
        })
        .sort((a, b) => (a.id < b.id ? 1 : -1))
      const items = hits.slice(0, SEARCH_PAGE_SIZE).map((message): SearchHit => {
        const ranges: [number, number][] = []
        const body = message.body.toLowerCase()
        for (let at = query ? body.indexOf(query) : -1; at >= 0; at = body.indexOf(query, at + query.length)) {
          ranges.push([at, at + query.length])
        }
        return { message, conversationId: message.conversationId, ranges }
      })
      return { items, cursor: hits.length > SEARCH_PAGE_SIZE ? items.at(-1)!.message.id : null }
    },

    async getPresence() {
      await wait()
      // The user is online; of the others, every third one is away.
      return [me, ...others.filter((_, index) => index % 3 !== 1)]
    },

    async sendMessage(input: SendMessageInput) {
      await wait()
      const conversation = memberConversationOf(input.conversationId)
      if (conversation.archived) throw new ChatError('forbidden', 'This channel is archived.')
      checkBody(input.body, input.attachments)
      if (input.threadRootId) {
        const root = messageOf(input.threadRootId)
        if (root.conversationId !== conversation.id || root.threadRootId !== null) {
          throw new ChatError('not_found', 'This thread no longer exists.')
        }
      }
      const message = addMessage({ ...input, authorId: me })
      simulateReply(message)
      return message
    },

    async editMessage(messageId, body) {
      await wait()
      const message = messageOf(messageId)
      if (message.deleted || message.kind !== 'message') throw new ChatError('not_found', 'This message no longer exists.')
      if (message.authorId !== me) throw new ChatError('forbidden', 'Only the author can edit a message.')
      checkBody(body, message.attachments)
      const mentions = extractMentions(body)
      if (message.threadRootId) Object.assign(mentions, { channel: false, here: false })
      const next: Message = { ...message, body, mentions, editedAt: Date.now() }
      putMessage(next)
      emit({ type: 'message.updated', message: next })
      const root = message.threadRootId ? messages.get(message.threadRootId) : undefined
      if (root && repliesOf(root).at(-1)?.id === next.id) {
        const nextRoot: Message = { ...root, lastReply: replyPreview(next) }
        putMessage(nextRoot)
        emit({ type: 'message.updated', message: nextRoot })
      }
      return next
    },

    async deleteMessage(messageId) {
      await wait()
      const message = messageOf(messageId)
      if (message.authorId !== me && !isAdmin) {
        throw new ChatError('forbidden', 'Only the author and workspace owners and admins can delete a message.')
      }
      if (message.threadRootId === null && message.replyCount > 0) {
        // A root with replies stays as "This message was deleted".
        const next: Message = { ...message, deleted: true, body: '', attachments: [], reactions: [], pinned: false }
        next.mentions = { userIds: [], channel: false, here: false }
        putMessage(next)
        emit({ type: 'message.updated', message: next })
      } else {
        removeMessage(message)
        const root = message.threadRootId ? messages.get(message.threadRootId) : undefined
        if (root) {
          const replies = repliesOf(root)
          if (root.deleted && replies.length === 0) {
            removeMessage(root)
          } else {
            const nextRoot: Message = {
              ...root,
              replyCount: replies.length,
              lastReplyAt: replies.at(-1)?.createdAt ?? null,
              replyUserIds: [...new Set(replies.map((reply) => reply.authorId))],
              lastReply: replyPreview(replies.at(-1)),
            }
            putMessage(nextRoot)
            emit({ type: 'message.updated', message: nextRoot })
            if (follows.has(root.id)) emitThreadState(root.id)
          }
        }
      }
      if (reads.has(message.conversationId)) emitState(message.conversationId)
    },

    async setReaction(messageId, emoji, on) {
      await wait()
      const message = messageOf(messageId)
      memberConversationOf(message.conversationId)
      if (hasReaction(message.reactions, emoji, me) === on) return message
      const next = { ...message, reactions: toggleReaction(message.reactions, emoji, me) }
      putMessage(next)
      emit({ type: 'message.updated', message: next })
      return next
    },

    async setPinned(messageId, pinned) {
      await wait()
      const message = messageOf(messageId)
      memberConversationOf(message.conversationId)
      if (message.pinned === pinned) return message
      const next = { ...message, pinned }
      putMessage(next)
      emit({ type: 'message.updated', message: next })
      if (pinned) addMessage({ conversationId: message.conversationId, authorId: me, kind: 'pin', body: '' })
      return next
    },

    async markRead(conversationId) {
      await wait()
      const read = reads.get(memberConversationOf(conversationId).id)!
      reads.set(conversationId, { ...read, lastReadMessageId: mainList(conversationId).at(-1)?.id ?? read.lastReadMessageId })
      return emitState(conversationId)
    },

    async markUnread(messageId) {
      await wait()
      const message = messageOf(messageId)
      const read = reads.get(memberConversationOf(message.conversationId).id)!
      if (message.threadRootId !== null && !message.alsoInChannel) {
        // A reply: the thread's cursor moves, and the thread becomes followed so that it can show as unread.
        const root = messageOf(message.threadRootId)
        const replies = repliesOf(root)
        const before = replies[replies.findIndex((reply) => reply.id === message.id) - 1]
        follows.set(root.id, { conversationId: root.conversationId, following: true, lastReadReplyId: before?.id ?? null })
        emitThreadState(root.id)
        return emitState(message.conversationId)
      }
      const list = mainList(message.conversationId)
      const before = list[list.findIndex((item) => item.id === message.id) - 1]
      reads.set(message.conversationId, { ...read, lastReadMessageId: before?.id ?? null })
      return emitState(message.conversationId)
    },

    async markThreadRead(rootId) {
      await wait()
      const root = messageOf(rootId)
      const follow = follows.get(rootId)
      follows.set(rootId, {
        conversationId: root.conversationId,
        following: follow?.following ?? false,
        lastReadReplyId: repliesOf(root).at(-1)?.id ?? null,
      })
      return emitThreadState(rootId)
    },

    async markAllRead() {
      await wait()
      const states = [...reads.keys()].flatMap((id) => stateOf(id) ?? []).filter((state) => state.unreadCount > 0)
      const threads = [...follows.keys()].flatMap((id) => threadStateOf(id) ?? []).filter((state) => state.unreadReplies > 0)
      for (const state of states) {
        const read = reads.get(state.conversationId)!
        reads.set(state.conversationId, { ...read, lastReadMessageId: mainList(state.conversationId).at(-1)?.id ?? null })
        emitState(state.conversationId)
      }
      for (const state of threads) {
        const root = messages.get(state.rootId)!
        follows.set(state.rootId, { ...follows.get(state.rootId)!, lastReadReplyId: repliesOf(root).at(-1)?.id ?? null })
        emitThreadState(state.rootId)
      }
      return { states, threads }
    },

    async restoreRead(previous) {
      await wait()
      for (const state of previous.states) {
        const read = reads.get(state.conversationId)
        if (!read) continue
        reads.set(state.conversationId, { ...read, lastReadMessageId: state.lastReadMessageId })
        emitState(state.conversationId)
      }
      for (const state of previous.threads) {
        const follow = follows.get(state.rootId)
        if (!follow) continue
        follows.set(state.rootId, { ...follow, lastReadReplyId: state.lastReadReplyId })
        emitThreadState(state.rootId)
      }
    },

    async setNotify(conversationId, notify) {
      await wait()
      const read = reads.get(memberConversationOf(conversationId).id)!
      reads.set(conversationId, { ...read, notify })
      return emitState(conversationId)
    },

    async setFavorite(conversationId, favorite) {
      await wait()
      const read = reads.get(memberConversationOf(conversationId).id)!
      reads.set(conversationId, { ...read, favorite })
      return emitState(conversationId)
    },

    async setThreadFollow(rootId, following) {
      await wait()
      const root = messageOf(rootId)
      const follow = follows.get(rootId)
      follows.set(rootId, {
        conversationId: root.conversationId,
        following,
        // Following starts from now: old replies do not turn unread.
        lastReadReplyId: following && !follow?.following ? (repliesOf(root).at(-1)?.id ?? null) : (follow?.lastReadReplyId ?? null),
      })
      return emitThreadState(rootId)
    },

    async createChannel(input: ChannelInput) {
      await wait()
      const name = normalizeName(input.name)
      if (input.categoryId && !categories.some((category) => category.id === input.categoryId)) {
        throw new ChatError('not_found', 'This category no longer exists.')
      }
      const conversation = addConversation({
        kind: input.kind,
        name,
        topic: input.topic?.trim(),
        categoryId: input.categoryId,
        memberIds: [me, ...(input.memberIds ?? [])],
        createdBy: me,
      })
      emit({ type: 'conversation.changed', conversation: view(conversation) })
      emitState(conversation.id)
      return view(conversation)
    },

    async updateChannel(conversationId, patch) {
      await wait()
      const conversation = conversationOf(conversationId)
      requireManage(conversation, 'edit a channel')
      if (conversation.isDefault && patch.kind === 'private') {
        throw new ChatError('forbidden', `#${conversation.name} is the default channel and cannot be private.`)
      }
      if (patch.categoryId && !categories.some((category) => category.id === patch.categoryId)) {
        throw new ChatError('not_found', 'This category no longer exists.')
      }
      const next: Conversation = {
        ...conversation,
        name: patch.name === undefined ? conversation.name : normalizeName(patch.name, conversation.id),
        topic: patch.topic === undefined ? conversation.topic : patch.topic.trim(),
        kind: patch.kind ?? conversation.kind,
        categoryId: patch.categoryId === undefined ? conversation.categoryId : patch.categoryId,
      }
      if (next.categoryId !== conversation.categoryId) {
        const siblings = [...conversations.values()].filter((item) => item.kind !== 'dm' && item.categoryId === next.categoryId)
        next.position = Math.max(-1, ...siblings.map((item) => item.position)) + 1
      }
      putConversation(next)
      return view(next)
    },

    async archiveChannel(conversationId) {
      await wait()
      const conversation = conversationOf(conversationId)
      requireManage(conversation, 'archive a channel')
      if (conversation.isDefault) {
        throw new ChatError('forbidden', `#${conversation.name} is the default channel and cannot be archived.`)
      }
      putConversation({ ...conversation, archived: true })
    },

    async joinChannel(conversationId) {
      await wait()
      const conversation = conversationOf(conversationId)
      if (isMember(conversation)) return view(conversation)
      if (conversation.archived) throw new ChatError('forbidden', 'This channel is archived.')
      const next = { ...conversation, memberIds: [...conversation.memberIds, me] }
      putConversation(next)
      startReading(next)
      addMessage({ conversationId, authorId: me, kind: 'join', body: '' })
      return view(conversations.get(conversationId)!)
    },

    async leaveChannel(conversationId) {
      await wait()
      const conversation = memberConversationOf(conversationId)
      if (conversation.isDefault) {
        throw new ChatError('forbidden', `#${conversation.name} is the default channel. Nobody can leave it.`)
      }
      if (conversation.kind === 'dm') throw new ChatError('forbidden', 'A direct message cannot be left.')
      addMessage({ conversationId, authorId: me, kind: 'leave', body: '' })
      const next = conversations.get(conversationId)!
      reads.delete(conversationId)
      conversations.set(conversationId, { ...next, memberIds: next.memberIds.filter((id) => id !== me) })
      // A public channel stays readable (Browse); a private one is gone for the user.
      if (next.kind === 'public') emit({ type: 'conversation.changed', conversation: view(conversations.get(conversationId)!) })
      else emit({ type: 'conversation.removed', conversationId })
    },

    async addMembers(conversationId, userIds) {
      await wait()
      const conversation = memberConversationOf(conversationId)
      if (conversation.kind === 'dm') throw new ChatError('forbidden', 'Start a new message to talk to more people.')
      if (conversation.kind === 'private') requireManage(conversation, 'add people to a private channel')
      if (userIds.some((id) => !options.members.some((member) => member.id === id))) {
        throw new ChatError('not_found', 'This person is not a member of the workspace.')
      }
      const added = [...new Set(userIds)].filter((id) => !conversation.memberIds.includes(id))
      putConversation({ ...conversation, memberIds: [...conversation.memberIds, ...added] })
      for (const userId of added) addMessage({ conversationId, authorId: userId, kind: 'join', body: '' })
      return view(conversations.get(conversationId)!)
    },

    async removeMember(conversationId, userId) {
      await wait()
      const conversation = conversationOf(conversationId)
      requireManage(conversation, 'remove people from a channel')
      if (conversation.isDefault) {
        throw new ChatError('forbidden', `#${conversation.name} is the default channel. Nobody can leave it.`)
      }
      if (!conversation.memberIds.includes(userId)) return view(conversation)
      addMessage({ conversationId, authorId: userId, kind: 'leave', body: '' })
      const next = { ...conversations.get(conversationId)!, memberIds: conversation.memberIds.filter((id) => id !== userId) }
      if (userId === me) reads.delete(conversationId)
      if (userId === me && next.kind === 'private') {
        conversations.set(conversationId, next)
        emit({ type: 'conversation.removed', conversationId })
      } else {
        putConversation(next)
      }
      return view(next)
    },

    async openDm(userIds) {
      await wait()
      const memberIds = [...new Set([me, ...userIds])]
      if (memberIds.some((id) => !options.members.some((member) => member.id === id))) {
        throw new ChatError('not_found', 'This person is not a member of the workspace.')
      }
      const existing = [...conversations.values()].find(
        (item) => item.kind === 'dm' && item.memberIds.length === memberIds.length && memberIds.every((id) => item.memberIds.includes(id)),
      )
      if (existing) return view(existing)
      const conversation = addConversation({ kind: 'dm', memberIds, createdBy: me })
      emit({ type: 'conversation.changed', conversation: view(conversation) })
      emitState(conversation.id)
      return view(conversation)
    },

    async createCategory(name) {
      await wait()
      requireAdmin('create categories')
      if (!name.trim()) throw new ChatError('conflict', 'A category needs a name.')
      const category: Category = {
        id: nextId('g'),
        name: name.trim(),
        position: Math.max(-1, ...categories.map((item) => item.position)) + 1,
      }
      categories = [...categories, category]
      emit({ type: 'categories.changed', categories })
      return category
    },

    async renameCategory(categoryId, name) {
      await wait()
      requireAdmin('rename categories')
      const category = categories.find((item) => item.id === categoryId)
      if (!category) throw new ChatError('not_found', 'This category no longer exists.')
      if (!name.trim()) throw new ChatError('conflict', 'A category needs a name.')
      const next = { ...category, name: name.trim() }
      categories = categories.map((item) => (item.id === categoryId ? next : item))
      emit({ type: 'categories.changed', categories })
      return next
    },

    async deleteCategory(categoryId) {
      await wait()
      requireAdmin('delete categories')
      if (!categories.some((item) => item.id === categoryId)) throw new ChatError('not_found', 'This category no longer exists.')
      categories = categories.filter((item) => item.id !== categoryId)
      const all = [...conversations.values()]
      let position = Math.max(-1, ...all.filter((item) => item.kind !== 'dm' && item.categoryId === null).map((item) => item.position))
      for (const conversation of all.filter((item) => item.categoryId === categoryId).sort((a, b) => a.position - b.position)) {
        const next = { ...conversation, categoryId: null, position: ++position }
        conversations.set(next.id, next)
        if (readable(next) && !next.archived) emit({ type: 'conversation.changed', conversation: view(next) })
      }
      emit({ type: 'categories.changed', categories })
    },

    async move(target, direction) {
      await wait()
      requireAdmin('reorder the channel list')
      const step = direction === 'up' ? -1 : 1
      if ('categoryId' in target) {
        const ordered = [...categories].sort((a, b) => a.position - b.position)
        const index = ordered.findIndex((item) => item.id === target.categoryId)
        if (index < 0) throw new ChatError('not_found', 'This category no longer exists.')
        const neighbour = ordered[index + step]
        if (!neighbour) return
        const moved = ordered[index]
        categories = categories.map((item) =>
          item.id === moved.id ? { ...item, position: neighbour.position } : item.id === neighbour.id ? { ...item, position: moved.position } : item,
        )
        emit({ type: 'categories.changed', categories })
        return
      }
      const moved = conversationOf(target.conversationId)
      // The neighbours the user sees: joined channels of the same category.
      const siblings = [...conversations.values()]
        .filter((item) => item.kind !== 'dm' && !item.archived && isMember(item) && item.categoryId === moved.categoryId)
        .sort((a, b) => a.position - b.position)
      const neighbour = siblings[siblings.findIndex((item) => item.id === moved.id) + step]
      if (moved.kind === 'dm' || !isMember(moved) || !neighbour) return
      putConversation({ ...moved, position: neighbour.position })
      putConversation({ ...neighbour, position: moved.position })
    },

    async uploadAttachment(file, { onProgress, signal } = {}) {
      const cancelled = () => new ChatError('upload_failed', 'The upload was cancelled.')
      for (const fraction of [0.25, 0.5, 0.75]) {
        if (signal?.aborted) throw cancelled()
        onProgress?.(fraction)
        await wait()
      }
      if (signal?.aborted) throw cancelled()
      let size: { width?: number; height?: number } = {}
      if (file.type.startsWith('image/') && typeof createImageBitmap === 'function') {
        try {
          const bitmap = await createImageBitmap(file)
          size = { width: bitmap.width, height: bitmap.height }
          bitmap.close()
        } catch {
          // Not decodable here (SVG, for one): the list shows it without reserved space.
        }
      }
      if (signal?.aborted) throw cancelled()
      onProgress?.(1)
      return {
        id: nextId('a'),
        fileName: file.name,
        mimeType: file.type || 'application/octet-stream',
        fileSize: file.size,
        // A blob: URL lives as long as the page, like the rest of the mock's state.
        url: URL.createObjectURL(file),
        ...size,
      }
    },

    // Nobody else is connected to the mock, so there is nobody to tell.
    sendTyping() {},

    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}
