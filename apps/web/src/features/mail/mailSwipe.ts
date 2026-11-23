import { useSyncExternalStore } from 'react'
import type { MailThreadSummary, UpdateMailThreadBody as ThreadChange } from '@orbit/api/generated/types.gen'

/** The swipe actions of the mail list on a touch screen, chosen in Settings → Mail → Mail preferences. */
export const SWIPE_ACTIONS = ['none', 'archive', 'trash', 'read', 'star', 'spam', 'reply', 'reply_all'] as const
export type SwipeAction = (typeof SWIPE_ACTIONS)[number]

/** `left` is a swipe of the finger to the left. */
export interface SwipeSettings {
  left: SwipeAction
  right: SwipeAction
}

export const DEFAULT_SWIPES: SwipeSettings = { left: 'archive', right: 'reply_all' }

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
  } catch {
    // a broken value: the defaults
  }
  return {
    left: isAction(stored.left) ? stored.left : DEFAULT_SWIPES.left,
    right: isAction(stored.right) ? stored.right : DEFAULT_SWIPES.right,
  }
}

/** What a swipe does to one thread: a change of the thread, or a reply under it. */
export type SwipeOutcome = { change: ThreadChange } | { reply: 'reply' | 'reply_all' }
export type SwipeIcon = 'archive' | 'inbox' | 'trash' | 'restore' | 'read' | 'unread' | 'star' | 'spam' | 'reply'

export interface SwipeStep {
  label: string
  icon: SwipeIcon
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
  const { folder } = thread
  switch (action) {
    case 'none':
      return null
    case 'archive':
      if (folder === 'inbox') return { label: 'Archive', icon: 'archive', color: GREEN, outcome: { change: { folder: 'archive' } }, leaves: true }
      if (folder === 'archive') return { label: 'Move to inbox', icon: 'inbox', color: GREEN, outcome: { change: { folder: 'inbox' } }, leaves: true }
      return null
    case 'trash':
      return folder === 'trash'
        ? { label: 'Restore', icon: 'restore', color: GREEN, outcome: { change: { restore: true } }, leaves: true }
        : { label: 'Trash', icon: 'trash', color: RED, outcome: { change: { folder: 'trash' } }, leaves: true }
    case 'read':
      return thread.unread
        ? { label: 'Read', icon: 'read', color: BLUE, outcome: { change: { read: true } }, leaves: false }
        : { label: 'Unread', icon: 'unread', color: BLUE, outcome: { change: { read: false } }, leaves: false }
    case 'star':
      return { label: thread.starred ? 'Unstar' : 'Star', icon: 'star', color: AMBER, outcome: { change: { starred: !thread.starred } }, leaves: false }
    case 'spam':
      if (folder === 'trash') return null
      return folder === 'spam'
        ? { label: 'Not spam', icon: 'spam', color: GREY, outcome: { change: { folder: 'inbox' } }, leaves: true }
        : { label: 'Spam', icon: 'spam', color: GREY, outcome: { change: { folder: 'spam' } }, leaves: true }
    case 'reply':
      return { label: 'Reply', icon: 'reply', color: BLUE, outcome: { reply: 'reply' }, leaves: false }
    case 'reply_all':
      return { label: 'Reply all', icon: 'reply', color: BLUE, outcome: { reply: 'reply_all' }, leaves: false }
  }
}

// The choice belongs to the device (only a touch screen swipes), so it is in `localStorage`.
const KEY = 'orbit:mail-swipes'
const listeners = new Set<() => void>()
let cached: { raw: string | null; swipes: SwipeSettings } | null = null

function stored(): SwipeSettings {
  let raw: string | null = null
  try {
    raw = localStorage.getItem(KEY)
  } catch {
    // private mode: the defaults
  }
  if (!cached || cached.raw !== raw) cached = { raw, swipes: parseSwipes(raw) }
  return cached.swipes
}

export function useMailSwipes(): SwipeSettings {
  return useSyncExternalStore((listener) => {
    listeners.add(listener)
    return () => listeners.delete(listener)
  }, stored)
}

export function setMailSwipes(next: SwipeSettings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(next))
  } catch {
    // private mode: the choice lasts this page
  }
  cached = { raw: JSON.stringify(next), swipes: next }
  for (const listener of listeners) listener()
}
