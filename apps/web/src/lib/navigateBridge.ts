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
