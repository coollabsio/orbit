export function dropdownPosition(
  trigger: { top: number; bottom: number; left: number; right: number },
  panel: { width: number; height: number },
  viewport: { top: number; bottom: number; left: number; width: number },
  align: 'left' | 'right',
  direction: 'down' | 'up',
) {
  const above = Math.max(0, trigger.top - viewport.top - 12)
  const below = Math.max(0, viewport.bottom - trigger.bottom - 12)
  const up = direction === 'up'
    ? above >= panel.height || above >= below
    : below < panel.height && above > below
  const maxHeight = up ? above : below
  const left = align === 'right' ? trigger.right - panel.width : trigger.left
  return {
    top: up ? trigger.top - Math.min(panel.height, maxHeight) - 4 : trigger.bottom + 4,
    left: Math.max(viewport.left + 8, Math.min(left, viewport.left + viewport.width - panel.width - 8)),
    maxHeight,
  }
}
