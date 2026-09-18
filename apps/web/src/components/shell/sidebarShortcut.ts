import { isBareKeyShortcut, useBareKeyShortcut } from './bareKeyShortcut'

/**
 * Linear collapses its sidebar with a bare `[`.
 * https://linear.app/changelog/unpublished-collapsible-sidebar
 */
export const SIDEBAR_TOGGLE_KEY = '['

export function isSidebarToggleShortcut(event: KeyboardEvent): boolean {
  return isBareKeyShortcut(event, SIDEBAR_TOGGLE_KEY)
}

/** Document-level `[` handler that toggles the sidebar. */
export function useSidebarToggleShortcut(onToggle: () => void) {
  useBareKeyShortcut(SIDEBAR_TOGGLE_KEY, onToggle)
}
