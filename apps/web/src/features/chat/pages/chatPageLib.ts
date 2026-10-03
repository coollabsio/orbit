import type { Conversation } from '@/features/chat/api/types'
import type { ChatLocation, ChatPane } from '@/features/chat/chatRoutes'

/** Widths of the chat layout, in px (spec "Layout and widths"). */
export const PANE_WIDTH = { min: 320, max: 560, initial: 400 } as const
/** The message column never gets narrower than this. */
export const COLUMN_MIN = 400

export interface PaneLayout {
  /** The right pane has room next to the message column. When false a thread takes the column and other panes are sheets. */
  fits: boolean
  /** The pane's width: the user's width, clamped so the column keeps its minimum. */
  width: number
}

/**
 * `available` is the width the message column and the right pane share (the page without the chat sidebar); `null`
 * before the first measurement, which is treated as wide enough so a desktop load does not flash the narrow layout.
 */
export function paneLayout(available: number | null, wanted: number): PaneLayout {
  const width = Math.round(Math.min(PANE_WIDTH.max, Math.max(PANE_WIDTH.min, wanted)))
  if (available === null) return { fits: true, width }
  if (available < COLUMN_MIN + PANE_WIDTH.min) return { fits: false, width: PANE_WIDTH.min }
  return { fits: true, width: Math.min(width, Math.floor(available - COLUMN_MIN)) }
}

/**
 * The Orbit task or page that a same-origin URL points to; `null` for every other URL. A task `id` is what the URL
 * names it by: its id or its identifier (`/tasks/ENG-12`, `/views/<view>/ENG-12`).
 */
export function orbitLinkTarget(url: string, origin: string): { kind: 'task' | 'page'; id: string } | null {
  let parsed: URL
  try {
    parsed = new URL(url, origin)
  } catch {
    return null
  }
  if (parsed.origin !== origin) return null
  const [, root, id, rest, more] = parsed.pathname.split('/')
  const segment = (value: string) => {
    try {
      return decodeURIComponent(value)
    } catch {
      return value
    }
  }
  if (root === 'views' && id && rest && !more) return { kind: 'task', id: segment(rest) }
  if (!id || rest) return null
  if (root === 'tasks') return id === 'projects' ? null : { kind: 'task', id: segment(id) }
  if (root === 'docs') return id === 'trash' || id === 'import' ? null : { kind: 'page', id }
  return null
}

/** The title of a task made from a message: its first line with text, cut at a word where possible. */
export function taskTitleFromMessage(text: string, limit = 80): string {
  const line = text.split('\n').map((part) => part.trim()).find(Boolean) ?? ''
  if (line.length <= limit) return line
  const cut = line.slice(0, limit)
  const space = cut.lastIndexOf(' ')
  return `${(space > limit * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`
}

export type RightPane =
  | { kind: 'thread'; rootId: string; focusMessageId: string | null }
  | { kind: 'search'; query: string }
  | { kind: ChatPane }

/** The one right pane a conversation URL asks for: a thread (`?thread=`) before a search (`?q=`) before `?pane=`. */
export function rightPaneOf(location: ChatLocation): RightPane | null {
  if (location.view !== 'conversation') return null
  if (location.thread) return { kind: 'thread', rootId: location.thread, focusMessageId: location.m }
  if (location.q) return { kind: 'search', query: location.q }
  return location.pane ? { kind: location.pane } : null
}

/**
 * The conversation `/chat` opens on a desktop: the last opened one while the user can still see it, else `#general`,
 * else the first joined one. `null` when there is none.
 */
export function homeConversation(conversations: readonly Conversation[], lastOpenedId: string | null): Conversation | null {
  const open = conversations.filter((conversation) => !conversation.archived)
  return (
    open.find((conversation) => conversation.id === lastOpenedId) ??
    open.find((conversation) => conversation.isDefault) ??
    open.find((conversation) => conversation.isMember) ??
    null
  )
}
