import { createContext, useContext } from 'react'

/** The place in the app sidebar for a page's own list. Set only while the desktop sidebar is open and has room. */
export const SidebarSlotContext = createContext<HTMLElement | null>(null)

/** Pages with their own list (the page tree, the conversations): on desktop it moves into the app sidebar. */
export function hostsPageList(pathname: string): boolean {
  return /^\/(docs|chat)(\/|$)/.test(pathname)
}

/** The page's list shows in the app sidebar, not beside it. */
export function useInAppSidebar(): boolean {
  return useContext(SidebarSlotContext) !== null
}
