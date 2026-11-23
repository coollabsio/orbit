/** The swipe actions of the mail list: what each direction does, chosen in Settings → Mail → Mail preferences. */
import type { MailThreadSummary } from '@orbit/api/generated/types.gen'
import type { IconName } from '../ui/Icon'
import { threadActions, type ThreadChange } from './mail'

export const SWIPE_ACTIONS = ['none', 'archive', 'trash', 'read', 'star', 'spam', 'reply', 'reply_all'] as const
export type SwipeAction = (typeof SWIPE_ACTIONS)[number]

/** `left` is a swipe of the finger to the left. */
export interface SwipeSettings {
  left: SwipeAction
  right: SwipeAction
}

export const DEFAULT_SWIPES: SwipeSettings = {
  left: 'archive',
  right: 'reply_all',
}

export const SWIPE_LABELS: Record<SwipeAction, string> = {
  none: 'Nothing',
  archive: 'Archive',
  trash: 'Move to trash',
  read: 'Mark as read or unread',
  star: 'Star or unstar',
  spam: 'Report spam',
  reply: 'Reply',
  reply_all: 'Reply all',
}

const isAction = (value: unknown): value is SwipeAction => SWIPE_ACTIONS.includes(value as SwipeAction)

/** The stored choice; a missing or unknown value keeps its default. */
export function parseSwipes(raw: string | null): SwipeSettings {
  let stored: Partial<Record<keyof SwipeSettings, unknown>> = {}
  try {
    stored = raw ? (JSON.parse(raw) ?? {}) : {}
  } catch {}
  return {
    left: isAction(stored.left) ? stored.left : DEFAULT_SWIPES.left,
    right: isAction(stored.right) ? stored.right : DEFAULT_SWIPES.right,
  }
}

/** What a swipe does to one thread: a change of the thread, or a reply in the compose screen. */
export type SwipeOutcome = { change: ThreadChange } | { reply: 'reply' | 'reply_all' }

export interface SwipeStep {
  label: string
  icon: IconName
  /** The colour behind the row while it slides. */
  color: string
  outcome: SwipeOutcome
  /** The thread leaves the open folder: the row slides out instead of back. */
  leaves: boolean
}

const GREEN = '#00a63e'
const BLUE = '#2b7fff'
const RED = '#e7000b'
const AMBER = '#e17100'
const GREY = '#62748e'

/** The action for this thread's folder, or `null` when it does nothing there (archive in the trash). */
export function swipeStep(action: SwipeAction, thread: Pick<MailThreadSummary, 'folder' | 'unread' | 'starred'>): SwipeStep | null {
  const actions = threadActions(thread.folder)
  switch (action) {
    case 'none':
      return null
    case 'archive':
      if (!actions.archive) return null
      return actions.archive === 'archive'
        ? {
            label: 'Archive',
            icon: 'archive',
            color: GREEN,
            outcome: { change: { folder: 'archive' } },
            leaves: true,
          }
        : {
            label: 'Move to inbox',
            icon: 'directInbox',
            color: GREEN,
            outcome: { change: { folder: 'inbox' } },
            leaves: true,
          }
    case 'trash':
      return actions.trash === 'trash'
        ? {
            label: 'Trash',
            icon: 'trash',
            color: RED,
            outcome: { change: { folder: 'trash' } },
            leaves: true,
          }
        : {
            label: 'Restore',
            icon: 'undo',
            color: GREEN,
            outcome: { change: { restore: true } },
            leaves: true,
          }
    case 'read':
      return thread.unread
        ? {
            label: 'Read',
            icon: 'mail',
            color: BLUE,
            outcome: { change: { read: true } },
            leaves: false,
          }
        : {
            label: 'Unread',
            icon: 'smsTracking',
            color: BLUE,
            outcome: { change: { read: false } },
            leaves: false,
          }
    case 'star':
      return {
        label: thread.starred ? 'Unstar' : 'Star',
        icon: 'star',
        color: AMBER,
        outcome: { change: { starred: !thread.starred } },
        leaves: false,
      }
    case 'spam':
      if (!actions.spam) return null
      return actions.spam === 'spam'
        ? {
            label: 'Spam',
            icon: 'shieldCross',
            color: GREY,
            outcome: { change: { folder: 'spam' } },
            leaves: true,
          }
        : {
            label: 'Not spam',
            icon: 'shieldCross',
            color: GREY,
            outcome: { change: { folder: 'inbox' } },
            leaves: true,
          }
    case 'reply':
      return {
        label: 'Reply',
        icon: 'reply',
        color: BLUE,
        outcome: { reply: 'reply' },
        leaves: false,
      }
    case 'reply_all':
      return {
        label: 'Reply all',
        icon: 'reply',
        color: BLUE,
        outcome: { reply: 'reply_all' },
        leaves: false,
      }
  }
}
