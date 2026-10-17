/** White or near-black text, whichever reads better on `color` (`#rrggbb`); WCAG relative luminance. */
export function inkOn(color: string): string {
  const channels = [1, 3, 5].map((start) => {
    const value = parseInt(color.slice(start, start + 2), 16) / 255
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  })
  const luminance = 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]
  return luminance <= 0.179 ? '#fff' : '#0d0d0d'
}

/**
 * Someone else's caret: BlockNote's markup and classes (its CSS shows the name flag while they type and on hover),
 * plus `data-collab-user` so the header avatars can scroll to it.
 */
export function renderCollabCursor(user: { id?: unknown; name: string; color: string }): HTMLElement {
  const style = `background-color: ${user.color}; color: ${inkOn(user.color)}`
  const cursor = document.createElement('span')
  cursor.className = 'bn-collaboration-cursor__base'
  if (typeof user.id === 'string') cursor.dataset.collabUser = user.id
  const caret = document.createElement('span')
  caret.className = 'bn-collaboration-cursor__caret'
  caret.contentEditable = 'false'
  caret.setAttribute('style', style)
  const label = document.createElement('span')
  label.className = 'bn-collaboration-cursor__label'
  label.setAttribute('style', style)
  label.textContent = user.name
  caret.append(label)
  // word joiners keep the caret from breaking the line around it
  cursor.append('⁠', caret, '⁠')
  return cursor
}

/** Scrolls to someone's caret in the open editor and shows their name flag. False when they have none. */
export function revealCollabCursor(root: ParentNode, userId: string): boolean {
  const cursor = root.querySelector<HTMLElement>(`.bn-collaboration-cursor__base[data-collab-user="${CSS.escape(userId)}"]`)
  if (!cursor) return false
  const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  cursor.scrollIntoView({ block: 'center', behavior: reduce ? 'auto' : 'smooth' })
  cursor.setAttribute('data-active', '')
  window.setTimeout(() => cursor.removeAttribute('data-active'), 2500)
  return true
}
