/** Height of the actually visible viewport, excluding iOS Safari chrome. */
export function visibleViewportHeight(
  viewport: { height: number } | null = typeof window === 'undefined' ? null : window.visualViewport,
  fallback = typeof window === 'undefined' ? 0 : window.innerHeight,
) {
  const height = viewport?.height ?? fallback
  return Math.max(0, Math.round(height))
}

export function applyVisibleViewport(
  root: { style: { setProperty(name: string, value: string): void } },
  height = visibleViewportHeight(),
) {
  root.style.setProperty('--app-height', `${height}px`)
}

/** Keep --app-height in sync when Safari shows or hides its toolbars. */
export function bindVisibleViewport(
  root: HTMLElement = document.documentElement,
  viewport: VisualViewport | null = window.visualViewport,
) {
  const apply = () => applyVisibleViewport(root, visibleViewportHeight(viewport, window.innerHeight))
  apply()
  viewport?.addEventListener('resize', apply)
  viewport?.addEventListener('scroll', apply)
  window.addEventListener('orientationchange', apply)
  return () => {
    viewport?.removeEventListener('resize', apply)
    viewport?.removeEventListener('scroll', apply)
    window.removeEventListener('orientationchange', apply)
  }
}
