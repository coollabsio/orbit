import { createContext, useContext } from 'react'

/** What a `page` block needs to know about its target page. `null` = the page does not exist (or is not loaded). */
export interface PageRef {
  title: string
  icon: string | null
  trashed: boolean
  /** A database page (a `page` block linking to it can turn into a database block). */
  database?: boolean
}

/** The page that holds database blocks: they read and change its databases through the workspace API. */
export interface DatabaseBlockHost {
  workspaceId: string
  pageId: string
  /** The page lock: the views of its linked view blocks are read-only. */
  locked: boolean
}

export interface PageEditorContextValue {
  resolvePage: (pageId: string) => PageRef | null
  onOpenPage: (pageId: string) => void
  /** Null where database blocks cannot load (read-only previews outside a docs page): they show a plain card. */
  databaseHost: DatabaseBlockHost | null
}

// Custom block views render through portals inside <BlockNoteView>, so they see this provider.
// The default covers HTML export paths that may render a block outside the editor tree.
export const PageEditorContext = createContext<PageEditorContextValue>({
  resolvePage: () => null,
  onOpenPage: () => {},
  databaseHost: null,
})

export function usePageEditorContext(): PageEditorContextValue {
  return useContext(PageEditorContext)
}

export type PageBlockState =
  | { kind: 'ok'; title: string; icon: string | null }
  | { kind: 'trashed'; title: string; icon: string | null }
  | { kind: 'missing' }

/** Maps the resolver result to what the card renders (kept pure for tests). */
export function pageBlockState(pageId: string, resolvePage: PageEditorContextValue['resolvePage']): PageBlockState {
  const page = pageId ? resolvePage(pageId) : null
  if (!page) return { kind: 'missing' }
  const title = page.title.trim() || 'Untitled'
  return page.trashed ? { kind: 'trashed', title, icon: page.icon } : { kind: 'ok', title, icon: page.icon }
}
