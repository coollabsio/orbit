import type { Message } from '../api/types'
import { isSameDay } from './time'

const CONTINUE_WITHIN = 5 * 60_000
const GROUP_SPAN = 10 * 60_000

export type MessageRow =
  | { type: 'day'; key: string; at: number }
  | { type: 'new'; key: 'new' }
  | { type: 'message'; key: string; message: Message; groupStart: boolean }

export interface MessageRowOptions {
  /** The read cursor captured when the conversation was opened. `undefined`: no "New" line. `null`: nothing was read. */
  lastReadMessageId?: string | null
  /** The user's own messages are never "new". */
  currentUserId?: string
}

/** Stable across the optimistic row and its confirmed message, so the row does not remount when the echo arrives. */
export function messageKey(message: Message): string {
  return message.nonce ?? message.id
}

/** A plain message that may share a group: not a system row, not deleted, not an "also sent to channel" reply. */
function groupable(message: Message): boolean {
  return message.kind === 'message' && !message.deleted && !message.alsoInChannel
}

/**
 * Messages (ascending) to render rows: a day separator before the first message of each local day, the "New" line
 * before the first unread message of another member, and for each message whether it starts a group (avatar, name,
 * time) or continues one.
 */
export function buildMessageRows(messages: readonly Message[], options: MessageRowOptions = {}): MessageRow[] {
  const { lastReadMessageId, currentUserId } = options
  const rows: MessageRow[] = []
  let previous: Message | null = null
  let groupStartedAt = 0
  let newLinePlaced = lastReadMessageId === undefined

  for (const message of messages) {
    const dayChanged = !previous || !isSameDay(previous.createdAt, message.createdAt)
    if (dayChanged) rows.push({ type: 'day', key: `day-${message.createdAt}`, at: message.createdAt })

    let isNew = false
    if (!newLinePlaced && message.authorId !== currentUserId && message.kind === 'message') {
      isNew = lastReadMessageId == null || message.id > lastReadMessageId
    }
    if (isNew) {
      rows.push({ type: 'new', key: 'new' })
      newLinePlaced = true
    }

    const continues =
      previous !== null &&
      !dayChanged &&
      !isNew &&
      groupable(previous) &&
      groupable(message) &&
      previous.authorId === message.authorId &&
      message.createdAt - previous.createdAt < CONTINUE_WITHIN &&
      message.createdAt - groupStartedAt < GROUP_SPAN
    if (!continues) groupStartedAt = message.createdAt

    rows.push({ type: 'message', key: messageKey(message), message, groupStart: !continues })
    previous = message
  }
  return rows
}
