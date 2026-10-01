import type { Conversation, NotifyLevel } from '@/features/chat/api/types'
import type { RowBadge, SidebarSection } from '@/features/chat/lib/sidebar'

/** Direct messages shown before "Show more". */
export const DM_LIMIT = 10

/** Every conversation of the sidebar, top to bottom. Collapsed sections and DMs behind "Show more" are included. */
export function sidebarOrder(sections: readonly SidebarSection[]): string[] {
  return sections.flatMap((section) => section.conversations.map((conversation) => conversation.id))
}

/**
 * The conversation before or after the open one that `matches` (all of them, or the unread ones). It wraps around at
 * the ends; with nothing open it starts from the top (next) or the bottom (previous). `null` when there is no other.
 */
export function adjacentConversation(
  ids: readonly string[],
  currentId: string | null,
  direction: 'previous' | 'next',
  matches: (id: string) => boolean = () => true,
): string | null {
  const step = direction === 'next' ? 1 : -1
  const index = currentId === null ? -1 : ids.indexOf(currentId)
  if (index === -1) {
    const pool = ids.filter(matches)
    return (direction === 'next' ? pool[0] : pool.at(-1)) ?? null
  }
  for (let offset = 1; offset < ids.length; offset += 1) {
    const id = ids[(index + step * offset + ids.length * offset) % ids.length]
    if (matches(id)) return id
  }
  return null
}

export interface VisibleRows {
  rows: Conversation[]
  /** Direct messages behind "Show more". */
  hidden: number
}

/**
 * The rows a section shows. A collapsed section keeps its unread rows and the open conversation; the DM section shows
 * the first ten (plus any unread or open one further down) until "Show more".
 */
export function visibleRows(
  section: SidebarSection,
  options: { collapsed: boolean; showAllDms: boolean; activeId: string | null; isUnread: (id: string) => boolean },
): VisibleRows {
  const kept = (conversation: Conversation) => conversation.id === options.activeId || options.isUnread(conversation.id)
  if (options.collapsed) return { rows: section.conversations.filter(kept), hidden: 0 }
  if (section.kind !== 'dms' || options.showAllDms) return { rows: section.conversations, hidden: 0 }
  const rows = section.conversations.filter((conversation, index) => index < DM_LIMIT || kept(conversation))
  return { rows, hidden: section.conversations.length - rows.length }
}

/** The accessible name of a sidebar row: "design, 3 unread mentions", "Ada Lovelace, 2 unread messages", "random, unread". */
export function rowLabel(title: string, badge: RowBadge, notify: NotifyLevel | undefined, draft = false): string {
  const parts = [title]
  if (badge.count > 0) {
    const noun = notify === 'all' ? 'message' : 'mention'
    parts.push(`${badge.count} unread ${noun}${badge.count === 1 ? '' : 's'}`)
  } else if (badge.bold) {
    parts.push('unread')
  }
  if (notify === 'muted') parts.push('muted')
  if (draft) parts.push('draft')
  return parts.join(', ')
}
