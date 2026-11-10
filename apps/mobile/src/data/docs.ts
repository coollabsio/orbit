/**
 * Docs as the web's sidebar tree (`features/docs/pageTree.ts`), a page's BlockNote blocks as readable lines, and the
 * plain paragraphs the app can add to a page. No React.
 */
import type { CreatePageBody, PageFavorite, PageKind } from '@orbit/api/generated/types.gen'

import { type MentionTarget, mentionInlineContent } from './mentions'
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

/**
 * A page's live sub-pages: from the tree for a tree page; a database row and its sub-pages are not in the tree, so
 * theirs come from `server` (`GET …/pages/{id}/descendants`).
 */
export async function subPagesOf(pages: readonly PageSummary[], pageId: string, server: (pageId: string) => Promise<PageSummary[]>): Promise<PageSummary[]> {
  return pages.some((page) => page.id === pageId) ? descendantsOf(pages, pageId) : server(pageId)
}

/** "Private", the teamspace's name, or "Teamspace" while the list is unknown. */
export function spaceLabel(space: string, teamspaces: readonly Pick<Teamspace, 'id' | 'name'>[] | undefined): string {
  if (space === 'private') return 'Private'
  return teamspaces?.find((teamspace) => `teamspace:${teamspace.id}` === space)?.name ?? 'Teamspace'
}

/** Where "Move to" sends a page: the end of a space's root pages, or the last sub-page (or row) of a page or database. */
export type MoveDestination = { kind: 'space'; space: string } | { kind: 'page'; page: PageSummary }

export interface MoveRequest {
  parent_id: string | null
  position: number
  private?: boolean
  teamspace_id?: string
}

/** The web's `destinationMove`: the `POST …/pages/{id}/move` body (without the version) for `destination`. */
export function moveRequest(pages: readonly PageSummary[], movingId: string, destination: MoveDestination): MoveRequest {
  if (destination.kind === 'page') {
    const siblings = pages.filter((page) => page.parent_id === destination.page.id && page.id !== movingId)
    return { parent_id: destination.page.id, position: siblings.length }
  }
  const roots = pages.filter((page) => page.parent_id === null && spaceOf(page) === destination.space && page.id !== movingId)
  const space = destination.space === 'private' ? { private: true } : { teamspace_id: destination.space.replace(/^teamspace:/, '') }
  return { parent_id: null, position: roots.length, ...space }
}

/** The search result's hint, as the web palette: "Row · Research" for a database row, else "Page · General". */
export function searchHint(result: { database_title: string | null; private: boolean; teamspace_id: string | null }, teamspaces: readonly Pick<Teamspace, 'id' | 'name'>[] | undefined): string {
  if (result.database_title !== null) return `Row · ${result.database_title || 'Untitled'}`
  return `Page · ${spaceLabel(result.private || !result.teamspace_id ? 'private' : `teamspace:${result.teamspace_id}`, teamspaces)}`
}

/** The web's "Move to trash" confirmation; `below` is the number of sub-pages that go with the page. */
export function trashPrompt(page: Pick<PageSummary, 'title'>, below: number): { title: string; description: string } {
  return {
    title: `Move “${pageTitle(page)}” to the trash?`,
    description: below > 0 ? `Its ${below} sub-page${below === 1 ? '' : 's'} move with it. You can restore them from Trash.` : 'You can restore it from Trash.',
  }
}

export interface FavoriteRow {
  id: string
  title: string
  icon: string | null
  kind: PageKind
  /** The database of a favorite row. */
  hint: string | null
  /** The tree summary; `null` for a page outside the tree (a database row or one of its sub-pages). */
  page: PageSummary | null
}

/**
 * The favorites in their order, from the server's entries: rows of databases are not in the tree. A tree page shows
 * its tree summary, which a save updates at once.
 */
export function favoritePages(pages: readonly PageSummary[], favorites: readonly PageFavorite[]): FavoriteRow[] {
  return favorites.map((favorite) => {
    const page = pages.find((item) => item.id === favorite.page_id) ?? null
    const shown = page ?? favorite
    return { id: favorite.page_id, title: shown.title, icon: shown.icon, kind: shown.kind, hint: favorite.database_title, page }
  })
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

/** One BlockNote paragraph per non-empty line, as the web editor stores typed text; `@Name` of `people` is a mention. */
export function paragraphBlocks(text: string, makeId: () => string = blockId, people: readonly MentionTarget[] = []): Record<string, unknown>[] {
  return text
    .split('\n')
    .map((line) => line.trimEnd())
    .filter((line) => line.trim() !== '')
    .map((line) => ({
      id: makeId(),
      type: 'paragraph',
      props: { backgroundColor: 'default', textAlignment: 'left', textColor: 'default' },
      content: mentionInlineContent(line, people),
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
