import { orbitLinkTarget } from '@/features/chat/pages/chatPageLib'

/**
 * What an element of an editor refers to, for its hover preview: a mention chip names a member (`user:<id>`), a link
 * to a task of this app names the task (`task:ENG-12` or `task:<id>`). Null for anything else.
 */
export function editorReference(element: Element, origin: string): string | null {
  const userId = element.getAttribute('data-mention-user')
  if (userId) return `user:${userId}`
  const href = element.tagName === 'A' ? element.getAttribute('href') : null
  const target = href ? orbitLinkTarget(href, origin) : null
  return target?.kind === 'task' ? `task:${target.id}` : null
}
