import { createContext, useContext } from 'react'

/** What a `page` block needs to know about its target page. `null` = the page does not exist (or is not loaded). */
export interface PageRef {
  title: string
  icon: string | null
  trashed: boolean
}

export interface PageEditorContextValue {
  resolvePage: (pageId: string) => PageRef | null
  onOpenPage: (pageId: string) => void
}

// Custom block views render through portals inside <BlockNoteView>, so they see this provider.
// The default covers HTML export paths that may render a block outside the editor tree.
export const PageEditorContext = createContext<PageEditorContextValue>({
  resolvePage: () => null,
  onOpenPage: () => {},
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
