import { isCoarsePointer } from './environment'

const ATTEMPTS = 12

/**
 * A thread closed: focus goes back to its root message in the conversation list, or to the conversation's composer
 * when that message is not on screen. The list may mount a few frames after the navigation, so this retries briefly.
 */
export function restoreFocusAfterThread(rootId: string) {
  let attempt = 0
  const tryFocus = () => {
    const view = document.querySelector('[data-slot="conversation-view"]')
    const row = view?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(rootId)}"]`)
    if (row) {
      row.focus({ preventScroll: true })
      return
    }
    attempt += 1
    if (attempt < ATTEMPTS) {
      requestAnimationFrame(tryFocus)
      return
    }
    // On a touch screen focusing the composer would open the keyboard.
    if (!isCoarsePointer()) view?.querySelector<HTMLElement>('[data-slot="composer"] textarea')?.focus()
  }
  requestAnimationFrame(tryFocus)
}
