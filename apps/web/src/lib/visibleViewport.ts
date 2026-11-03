/**
 * A real keyboard is much taller than this. The iOS installed-app bug
 * (WebKit 297779 / 313800) only shortens the visual viewport by about one
 * status bar, which leaves an empty band under the page.
 */
const KEYBOARD_SHORTFALL = 120

export function isStandaloneDisplay(
  matches: (query: string) => boolean = (query) => typeof window !== 'undefined' && window.matchMedia(query).matches,
  iosStandalone = typeof navigator !== 'undefined' && (navigator as { standalone?: boolean }).standalone === true,
) {
  return iosStandalone || matches('(display-mode: standalone)')
}

/**
 * Height of the app shell.
 * In a browser tab, use the visual viewport so Safari toolbars do not cover the dock.
 * In an installed app, `visualViewport` and `svh` stay short by about the status bar
 * even though `100vh` fills the window (viewport-fit=cover + black-translucent).
 * Follow the visual viewport there only when the keyboard is open.
 */
export function shellHeight(input: {
  standalone: boolean
  visual: number | null
  inner: number
  large: number
}) {
  const visual = input.visual ?? input.inner
  if (!input.standalone) return Math.max(0, Math.round(visual))
  if (keyboardOpen(input)) return Math.max(0, Math.round(visual))
  return Math.round(Math.max(input.inner, input.large, 0))
}

/** The on-screen keyboard covers the page: the visual viewport is much shorter than the window. */
export function keyboardOpen(input: { visual: number | null; inner: number; large: number }) {
  return Math.max(input.inner, input.large, 0) - (input.visual ?? input.inner) > KEYBOARD_SHORTFALL
}

/** Height of the actually visible viewport, excluding iOS Safari chrome. */
export function visibleViewportHeight(
  viewport: { height: number } | null = typeof window === 'undefined' ? null : window.visualViewport,
  fallback = typeof window === 'undefined' ? 0 : window.innerHeight,
) {
  return shellHeight({ standalone: false, visual: viewport?.height ?? null, inner: fallback, large: fallback })
}

export function applyVisibleViewport(
  root: { style: { setProperty(name: string, value: string): void } },
  height = visibleViewportHeight(),
) {
  root.style.setProperty('--app-height', `${height}px`)
}

function largeViewportProbe() {
  const probe = document.createElement('div')
  probe.setAttribute('aria-hidden', 'true')
  probe.style.cssText = 'position:fixed;top:0;height:100vh;width:0;visibility:hidden;pointer-events:none'
  document.documentElement.appendChild(probe)
  return probe
}

/** The ancestors that scroll vertically, nearest first, up to (not including) the page. */
function scrollingPanes(element: HTMLElement): HTMLElement[] {
  const panes: HTMLElement[] = []
  for (let parent = element.parentElement; parent && parent !== document.body && parent !== document.documentElement; parent = parent.parentElement) {
    const overflow = getComputedStyle(parent).overflowY
    if ((overflow === 'auto' || overflow === 'scroll') && parent.scrollHeight > parent.clientHeight) panes.push(parent)
  }
  return panes
}

/** Scrolls one pane by the least amount that brings the field into view (like `block: 'nearest'`). */
export function scrollIntoPane(field: HTMLElement, pane: HTMLElement, margin = 0) {
  const box = field.getBoundingClientRect()
  const view = pane.getBoundingClientRect()
  if (box.bottom + margin > view.bottom) pane.scrollTop += Math.min(box.bottom + margin - view.bottom, box.top - view.top)
  else if (box.top < view.top) pane.scrollTop -= view.top - box.top
}

/**
 * `scrollIntoView` for a field above the keyboard, without panning the page: iOS Safari would pan it too, which leaves
 * the short shell above an empty band. Every scrolling pane around the field moves, nearest first.
 */
export function revealField(field: HTMLElement) {
  for (const pane of scrollingPanes(field)) scrollIntoPane(field, pane, 12)
  if (window.scrollY || window.visualViewport?.offsetTop) window.scrollTo(0, 0)
}

/**
 * Keep --app-height in sync when Safari shows or hides its toolbars or the keyboard.
 * `data-keyboard` on the root marks an open keyboard. When the shell gets shorter, the
 * focused field scrolls back into its pane; Safari only pans the outer document to it.
 */
export function bindVisibleViewport(
  root: HTMLElement = document.documentElement,
  viewport: VisualViewport | null = window.visualViewport,
) {
  const probe = largeViewportProbe()
  let previous = 0
  const apply = () => {
    const input = {
      standalone: isStandaloneDisplay(),
      visual: viewport?.height ?? null,
      inner: window.innerHeight,
      large: probe.getBoundingClientRect().height || window.innerHeight,
    }
    const height = shellHeight(input)
    applyVisibleViewport(root, height)
    root.toggleAttribute('data-keyboard', keyboardOpen(input))
    const field = document.activeElement
    if (height < previous && field instanceof HTMLElement && field.matches('input, textarea, [contenteditable="true"]')) {
      // after the shorter shell has laid out
      requestAnimationFrame(() => revealField(field))
    }
    previous = height
  }
  // A tapped field comes into view at once, before anything is typed, and again when the keyboard has finished
  // opening (iOS reports the shorter viewport in steps, and sometimes only at the end).
  let settle: ReturnType<typeof setTimeout> | undefined
  const onFocus = (event: FocusEvent) => {
    const field = event.target
    if (!(field instanceof HTMLElement) || !field.matches('input, textarea, [contenteditable="true"]')) return
    if (!window.matchMedia('(pointer: coarse)').matches) return
    requestAnimationFrame(() => revealField(field))
    clearTimeout(settle)
    settle = setTimeout(() => {
      if (document.activeElement === field) revealField(field)
    }, 400)
  }
  apply()
  document.addEventListener('focusin', onFocus)
  viewport?.addEventListener('resize', apply)
  window.addEventListener('resize', apply)
  window.addEventListener('orientationchange', apply)
  return () => {
    probe.remove()
    document.removeEventListener('focusin', onFocus)
    clearTimeout(settle)
    viewport?.removeEventListener('resize', apply)
    window.removeEventListener('resize', apply)
    window.removeEventListener('orientationchange', apply)
  }
}
