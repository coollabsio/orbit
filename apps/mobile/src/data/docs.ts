/**
 * Docs as the web's sidebar tree (`features/docs/pageTree.ts`), a page's BlockNote blocks as readable lines, and the
 * plain paragraphs the app can add to a page. No React.
 */
import type { CreatePageBody } from '@orbit/api/generated/types.gen'

import type { PageSummary, Teamspace } from './orbit'

export const pageTitle = (page: Pick<PageSummary, 'title'>) => page.title.trim() || 'Untitled'

export interface TreeRow {
  page: PageSummary
  depth: number
  hasChildren: boolean
}

export interface DocSpace {
  /** `teamspace:<id>` or `private`, as the web keys a space. */
  key: string
  name: string
  rows: TreeRow[]
}

const byPosition = (a: PageSummary, b: PageSummary) => a.position - b.position || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
const spaceOf = (page: PageSummary) => (page.private || !page.teamspace_id ? 'private' : `teamspace:${page.teamspace_id}`)

/**
 * Teamspaces by position, then Private: each with its pages depth first in the server's order. The children of a page
 * in `collapsed` are left out.
 */
export function docSpaces(pages: readonly PageSummary[], teamspaces: readonly Teamspace[], collapsed: ReadonlySet<string>): { teamspaces: DocSpace[]; private: DocSpace } {
  const children = (parentId: string) => pages.filter((page) => page.parent_id === parentId).sort(byPosition)
  const rowsOf = (roots: PageSummary[], depth: number): TreeRow[] =>
    roots.flatMap((page) => {
      const below = children(page.id)
      return [{ page, depth, hasChildren: below.length > 0 }, ...(collapsed.has(page.id) ? [] : rowsOf(below, depth + 1))]
    })
  const space = (key: string, name: string): DocSpace => ({ key, name, rows: rowsOf(pages.filter((page) => page.parent_id === null && spaceOf(page) === key).sort(byPosition), 0) })
  return {
    teamspaces: [...teamspaces].sort((a, b) => a.position - b.position).map((teamspace) => space(`teamspace:${teamspace.id}`, teamspace.name)),
    private: space('private', 'Private'),
  }
}

/** Where a new page goes: a teamspace, Private, or (no space) the server's default teamspace. */
export function spaceRequest(space: string | null): CreatePageBody {
  if (space === null) return {}
  if (space === 'private') return { private: true }
  return { teamspace_id: space.replace(/^teamspace:/, '') }
}

/** Every page below `pageId`, at any depth. */
export function descendantsOf(pages: readonly PageSummary[], pageId: string): PageSummary[] {
  const out: PageSummary[] = []
  const queue = [pageId]
  while (queue.length > 0) {
    const parent = queue.shift()
    for (const page of pages) {
      if (page.parent_id === parent && !out.includes(page)) {
        out.push(page)
        queue.push(page.id)
      }
    }
  }
  return out
}

/** The web's "Move to trash" confirmation. */
export function trashPrompt(pages: readonly PageSummary[], page: PageSummary): { title: string; description: string } {
  const below = descendantsOf(pages, page.id).length
  return {
    title: `Move “${pageTitle(page)}” to the trash?`,
    description: below > 0 ? `Its ${below} sub-page${below === 1 ? '' : 's'} move with it. You can restore them from Trash.` : 'You can restore it from Trash.',
  }
}

/** The favorite pages that still exist, in the favorites' order. */
export function favoritePages(pages: readonly PageSummary[], ids: readonly string[]): PageSummary[] {
  return ids.flatMap((id) => pages.find((page) => page.id === id) ?? [])
}

/** The web's cover URL rule: an http(s) URL up to 2048 characters. */
export function validCoverUrl(raw: string): string | null {
  const url = raw.trim()
  if (url.length === 0 || url.length > 2048) return null
  return /^https?:\/\/[^\s/]+/i.test(url) ? url : null
}

/** A page's own file URLs are paths on the server; others are absolute. */
export const absoluteUrl = (server: string, url: string) => (url.startsWith('/') ? `${server}${url}` : url)

/** A random v4 UUID: the id BlockNote gives a block. */
export function blockId(random: () => number = Math.random): string {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(random() * 16)
    return (char === 'x' ? value : (value & 0x3) | 0x8).toString(16)
  })
}

/** One BlockNote paragraph per non-empty line, as the web editor stores typed text. */
export function paragraphBlocks(text: string, makeId: () => string = blockId): Record<string, unknown>[] {
  return text
    .split('\n')
    .map((line) => line.trimEnd())
    .filter((line) => line.trim() !== '')
    .map((line) => ({
      id: makeId(),
      type: 'paragraph',
      props: { backgroundColor: 'default', textAlignment: 'left', textColor: 'default' },
      content: [{ type: 'text', text: line, styles: {} }],
      children: [],
    }))
}

/** Emoji offered for a page or teamspace icon; any other one can be typed. */
export const PAGE_ICONS = ['📄', '📝', '📌', '📎', '📚', '📊', '📈', '🗂️', '🗓️', '✅', '💡', '🚀', '🎯', '🧭', '🛠️', '⚙️', '🔒', '🌱', '🔥', '⭐', '❤️', '🎉', '🧪', '🐛'] as const

/** A typed icon: its first emoji (a whole ZWJ sequence), or `null` when it is empty. */
export function iconOf(input: string): string | null {
  const trimmed = input.trim()
  if (!trimmed) return null
  // Hermes may lack `Intl.Segmenter`: then the first 16 code points, enough for any emoji sequence
  if (typeof Intl.Segmenter !== 'function') return Array.from(trimmed).slice(0, 16).join('')
  for (const part of new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment(trimmed)) return part.segment
  return null
}

interface ThreadLike {
  resolved: boolean
  updated_at: string
  comments: readonly { deleted_at?: string | null }[]
}

/** Threads with a comment left, open ones first, the latest first. */
export function visibleThreads<T extends ThreadLike>(threads: readonly T[]): T[] {
  return threads
    .filter((thread) => thread.comments.some((comment) => !comment.deleted_at))
    .sort((a, b) => Number(a.resolved) - Number(b.resolved) || b.updated_at.localeCompare(a.updated_at))
}
