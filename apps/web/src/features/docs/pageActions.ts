import { createContext, useContext } from 'react'

/** Page actions DocsPage owns (they need the open editor and navigation), offered to the tree rows and page header. */
export interface DocPageActions {
  /** Copies the page (with its sub-pages when `includeChildren`), then opens the copy. */
  duplicate: (pageId: string, includeChildren: boolean) => void
}

export const DocPageActionsContext = createContext<DocPageActions | null>(null)

/** `null` outside DocsPage: callers hide the actions. */
export function useDocPageActions(): DocPageActions | null {
  return useContext(DocPageActionsContext)
}
