// Tab picks the highlighted entry of an open suggestion menu ("@", "/", ":"), as in Discord. BlockNote's menus only
// take Enter, and Tab would indent the block or leave the editor.

/** Whether a BlockNote suggestion menu is open on this editor (the menu sets these on the editable element). */
function suggestionMenuOpen(element: HTMLElement) {
  return element.getAttribute('aria-expanded') === 'true' && element.getAttribute('aria-controls') === 'bn-suggestion-menu'
}

/** A keydown listener for `element`: while a suggestion menu is open, plain Tab acts as Enter does there. */
export function completeOnTab(element: HTMLElement) {
  return (event: KeyboardEvent) => {
    if (event.key !== 'Tab' || event.shiftKey || event.altKey || event.ctrlKey || event.metaKey || event.isComposing) return
    if (!suggestionMenuOpen(element)) return
    event.preventDefault()
    event.stopImmediatePropagation()
    // the menu's own key handler (on this element) picks the highlighted entry on Enter
    element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true }))
  }
}
