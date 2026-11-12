import { parsePullParam, pullParam, type PullRef } from '@orbit/api/github'

/** The search of the current page with a pull request open over it (`?pr=owner/repo/n`, and the comment to show). */
export function withPull(search: string, ref: PullRef, commentId?: number): string {
  const params = new URLSearchParams(search)
  params.delete('pr')
  params.delete('prComment')
  // the slashes stay readable in a shared link: `?pr=acme/orbit/482`
  const pr = pullParam(ref).split('/').map(encodeURIComponent).join('/')
  const rest = params.toString()
  return `?${rest ? `${rest}&` : ''}pr=${pr}${commentId === undefined ? '' : `&prComment=${commentId}`}`
}

/** The search of the page under the overlay. */
export function withoutPull(search: string): string {
  const params = new URLSearchParams(search)
  params.delete('pr')
  params.delete('prComment')
  const rest = params.toString()
  return rest ? `?${rest}` : ''
}

/** The pull request a search opens, and the review comment to show in it. */
export function pullFromSearch(search: string): { ref: PullRef; commentId: number | null } | null {
  const params = new URLSearchParams(search)
  const ref = parsePullParam(params.get('pr'))
  if (!ref) return null
  const comment = Number(params.get('prComment'))
  return { ref, commentId: Number.isSafeInteger(comment) && comment > 0 ? comment : null }
}
