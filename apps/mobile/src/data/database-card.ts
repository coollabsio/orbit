/**
 * The database blocks of a page on a phone (spec 3.7): the page editor (a DOM component) reports which databases its
 * blocks show, the native screen fetches each and hands back what its card says. No React and no runtime imports, so
 * the editor's web bundle can use it too.
 */
import type { DatabaseDetail } from '@orbit/api/generated/types.gen'

/** What a `database` block points to: `linkId` is empty for the database's own views, else a linked view. */
export interface DatabaseBlockRef {
  databaseId: string
  linkId: string
}

/** What a database card shows. */
export type DatabaseCardInfo =
  | { state: 'loading' }
  | { state: 'ready'; title: string; icon: string | null; rowCount: number; view: { id: string; name: string } | null }
  | { state: 'trashed'; title: string; icon: string | null }
  | { state: 'missing' }
  | { state: 'no-access' }
  | { state: 'link-gone' }
  | { state: 'failed' }

/** The key of a block's card in the lookup the screen passes to the editor. */
export const databaseCardKey = ({ databaseId, linkId }: DatabaseBlockRef) => `${databaseId}:${linkId}`

/** The distinct database blocks of a BlockNote document, nested blocks included, in document order. */
export function databaseBlocks(blocks: readonly DocumentBlock[]): DatabaseBlockRef[] {
  const found = new Map<string, DatabaseBlockRef>()
  const walk = (list: readonly DocumentBlock[]) => {
    for (const block of list) {
      if (block.type === 'database') {
        const ref = { databaseId: String(block.props?.databaseId ?? ''), linkId: String(block.props?.linkId ?? '') }
        if (ref.databaseId) found.set(databaseCardKey(ref), ref)
      }
      if (block.children) walk(block.children)
    }
  }
  walk(blocks)
  return [...found.values()]
}

interface DocumentBlock {
  type: string
  props?: Record<string, unknown>
  children?: readonly DocumentBlock[]
}

/** One block's fetch of `GET /databases/{id}` (with `?link=` for a linked view). */
export interface DatabaseFetch {
  data: DatabaseDetail | undefined
  /** The failure, with the `status` and `code` of an Orbit problem when there is one. */
  error: unknown
}

/**
 * The card of a block. The server answers 404 for a trashed database and for one the viewer cannot read, so the trash
 * tells the first apart; a 404 for a page the viewer can read means it is not a database.
 */
export function databaseCard(
  ref: DatabaseBlockRef,
  fetch: DatabaseFetch | undefined,
  pages: { trashed: ReadonlyMap<string, { title: string; icon: string | null }>; readable: ReadonlySet<string> },
): DatabaseCardInfo {
  if (!ref.databaseId) return { state: 'missing' }
  const trashed = pages.trashed.get(ref.databaseId)
  if (trashed) return { state: 'trashed', title: trashed.title, icon: trashed.icon }
  if (fetch?.data) {
    const { database, views } = fetch.data
    const first = [...views].sort((a, b) => a.position - b.position)[0]
    return { state: 'ready', title: database.title, icon: database.icon, rowCount: database.row_count, view: first ? { id: first.id, name: first.name } : null }
  }
  if (!fetch?.error) return { state: 'loading' }
  const { status, code } = problemOf(fetch.error)
  if (status !== 404) return { state: 'failed' }
  if (code === 'database_link_not_found') return { state: 'link-gone' }
  return pages.readable.has(ref.databaseId) ? { state: 'missing' } : { state: 'no-access' }
}

function problemOf(error: unknown): { status?: unknown; code?: unknown } {
  return typeof error === 'object' && error !== null ? (error as { status?: unknown; code?: unknown }) : {}
}

/** The database screen of a card: the block's linked view (with `hostId`, the page that shows it), opened on the view the card shows. */
export function databaseHref(ref: DatabaseBlockRef, viewId: string | null, hostId?: string): string {
  const params = new URLSearchParams()
  if (ref.linkId) params.set('link', ref.linkId)
  if (ref.linkId && hostId) params.set('host', hostId)
  if (viewId) params.set('view', viewId)
  const query = params.toString()
  return `/docs/${encodeURIComponent(ref.databaseId)}${query ? `?${query}` : ''}`
}
