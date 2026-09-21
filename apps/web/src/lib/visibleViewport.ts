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
  const full = Math.max(input.inner, input.large, 0)
  if (full - visual > KEYBOARD_SHORTFALL) return Math.max(0, Math.round(visual))
  return Math.round(full)
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

/** Keep --app-height in sync when Safari shows or hides its toolbars. */
export function bindVisibleViewport(
  root: HTMLElement = document.documentElement,
  viewport: VisualViewport | null = window.visualViewport,
) {
  const probe = largeViewportProbe()
  const apply = () => {
    const large = probe.getBoundingClientRect().height || window.innerHeight
    applyVisibleViewport(root, shellHeight({
      standalone: isStandaloneDisplay(),
      visual: viewport?.height ?? null,
      inner: window.innerHeight,
      large,
    }))
  }
  apply()
  viewport?.addEventListener('resize', apply)
  window.addEventListener('resize', apply)
  window.addEventListener('orientationchange', apply)
  return () => {
    probe.remove()
    viewport?.removeEventListener('resize', apply)
    window.removeEventListener('resize', apply)
    window.removeEventListener('orientationchange', apply)
  }
}
