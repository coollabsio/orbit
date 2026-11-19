/**
 * The app routes of the pull request screens. They live outside the tabs (`app/pulls`, a full-screen modal), so a
 * pull request opened from chat or the inbox opens over that tab. The web keeps `/tasks/pulls/…` (`pullPath`), which
 * the server still sends in notifications: `pushRoute` maps it here. No React.
 */
import type { PullRef } from '@orbit/api/github'

/** A pull request's screen; with `commentId` (a review comment's database id) it opens that thread. */
export function pullRoute(ref: PullRef, commentId?: number): string {
  const path = `/pulls/${encodeURIComponent(ref.owner)}/${encodeURIComponent(ref.repo)}/${ref.number}`
  return commentId === undefined ? path : `${path}?comment=${commentId}`
}

/** The diff screen of one file; `thread` is the thread to show (from a notification). */
export function fileHref(pull: { repository: string; number: number }, path: string, thread?: string): string {
  const [owner = '', repo = ''] = pull.repository.split('/')
  const query = `path=${encodeURIComponent(path)}${thread ? `&thread=${encodeURIComponent(thread)}` : ''}`
  return `${pullRoute({ owner, repo, number: pull.number })}/file?${query}`
}
