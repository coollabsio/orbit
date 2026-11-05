// The markdown renderer (plain functions, no hooks) needs to push routes when a channel
// mention or an in-app link is clicked. App mounts the router's navigate function here.
let navigateFn: ((to: string) => void) | null = null

export function setAppNavigate(fn: (to: string) => void) {
  navigateFn = fn
}

export function appNavigate(to: string) {
  if (navigateFn) navigateFn(to)
  else window.location.assign(to)
}

/** The in-app path of a link to this app (same origin), or null for any other link. */
export function internalPath(url: string): string | null {
  const origin = window.location.origin
  return url.startsWith(`${origin}/`) ? url.slice(origin.length) : null
}

/** A plain left click (no modifier asks for a new tab or window). */
export function isPlainClick(event: { button: number; metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean }): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey
}
