const OPEN_POPUP = ['menu', 'dialog', 'alertdialog', 'listbox'].map((role) => `[role="${role}"]:not([data-closed])`).join()

/** Single-key shortcuts stay quiet while typing, with modifiers held, or while any popup is open. */
export function shouldIgnoreShortcut(event: KeyboardEvent): boolean {
  if (event.defaultPrevented || event.repeat || event.metaKey || event.ctrlKey || event.altKey) return true
  if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]')) return true
  return document.querySelector(OPEN_POPUP) !== null
}
