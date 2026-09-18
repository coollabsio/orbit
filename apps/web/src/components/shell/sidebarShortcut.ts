import { useEffect } from 'react'

/**
 * Linear collapses its sidebar with a bare `[`.
 * https://linear.app/changelog/unpublished-collapsible-sidebar
 */
export const SIDEBAR_TOGGLE_KEY = '['

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

/** A bare printable key, so it only counts outside text entry and without modifiers. */
export function isSidebarToggleShortcut(event: KeyboardEvent): boolean {
  if (event.key !== SIDEBAR_TOGGLE_KEY) return false
  if (event.metaKey || event.ctrlKey || event.altKey) return false
  if (event.defaultPrevented) return false
  return !isEditableTarget(event.target)
}

/** Document-level `[` handler that toggles the sidebar. */
export function useSidebarToggleShortcut(onToggle: () => void) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isSidebarToggleShortcut(event)) return
      event.preventDefault()
      onToggle()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onToggle])
}
