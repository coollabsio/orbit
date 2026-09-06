export function listboxPosition(
  trigger: { top: number; bottom: number; left: number; width: number },
  contentHeight: number,
  viewport: { top: number; bottom: number; left: number; width: number },
) {
  const below = Math.max(0, viewport.bottom - trigger.bottom - 12)
  const above = Math.max(0, trigger.top - viewport.top - 12)
  const up = below < Math.min(contentHeight, 256) && above > below
  const maxHeight = Math.min(256, up ? above : below)
  const width = Math.min(Math.max(trigger.width, 208), viewport.width - 16)
  return {
    top: up ? trigger.top - Math.min(contentHeight, maxHeight) - 4 : trigger.bottom + 4,
    left: Math.max(viewport.left + 8, Math.min(trigger.left, viewport.left + viewport.width - width - 8)),
    width,
    maxHeight,
  }
}
