/** GitHub pull requests in Orbit: their URLs, Orbit paths and patches. Shared by the web and mobile apps. No React. */

export type PullRef = { owner: string; repo: string; number: number }

/** Where a review comment goes: GitHub's side (`LEFT` = old file, `RIGHT` = new file), line and optional range start. */
export type CommentTarget = { path: string; side: 'LEFT' | 'RIGHT'; line: number; start_side?: 'LEFT' | 'RIGHT'; start_line?: number }

const NAME = /^(?!\.{1,2}$)[A-Za-z0-9_.-]{1,100}$/

/** `https://github.com/o/r/pull/12[/files][#…]` → its parts; anything else → `null`. */
export function parseGithubPullUrl(url: string): PullRef | null {
  const match = /^https?:\/\/(?:www\.)?github\.com\/([^/?#]+)\/([^/?#]+)\/pull\/(\d+)(?:[/?#]|$)/i.exec(url.trim())
  if (!match) return null
  const [, owner, repo, digits] = match
  const number = Number(digits)
  if (!NAME.test(owner) || !NAME.test(repo) || !Number.isSafeInteger(number) || number <= 0) return null
  return { owner, repo, number }
}

/** The Orbit path of a pull request; with `commentId` (a review comment's database id) it opens that thread. */
export function pullPath(ref: PullRef, commentId?: number): string {
  const path = `/tasks/pulls/${encodeURIComponent(ref.owner)}/${encodeURIComponent(ref.repo)}/${ref.number}`
  return commentId === undefined ? path : `${path}?comment=${commentId}`
}

/** GitHub sends only the hunks of a file; diff renderers want a git patch. `null` when GitHub sent no hunks. */
export function toGitPatch(file: { path: string; previous_path?: string | null; status: string; patch?: string | null }): string | null {
  if (file.patch == null) return null
  const before = file.previous_path ?? file.path
  const lines = [`diff --git a/${before} b/${file.path}`]
  if (file.status === 'added') lines.push('new file mode 100644', '--- /dev/null', `+++ b/${file.path}`)
  else if (file.status === 'removed') lines.push('deleted file mode 100644', `--- a/${before}`, '+++ /dev/null')
  else {
    if (file.status === 'renamed' && before !== file.path) lines.push(`rename from ${before}`, `rename to ${file.path}`)
    lines.push(`--- a/${before}`, `+++ b/${file.path}`)
  }
  return `${lines.join('\n')}\n${file.patch.replace(/\n$/, '')}\n`
}

/** Review threads the diff cannot show on a line: outdated, on a whole file, or on a file without a diff. */
export function threadsOutsideDiff<T extends { path: string; line?: number | null }>(threads: T[], files: { path: string; patch?: string | null }[]): T[] {
  const shown = new Set(files.filter((file) => file.patch != null).map((file) => file.path))
  return threads.filter((thread) => thread.line == null || !shown.has(thread.path))
}

/** A line range as the diff renderer reports it: where the drag started and ended, on which side. */
export type LineRange = { start: number; end: number; side?: 'additions' | 'deletions'; endSide?: 'additions' | 'deletions' }

const githubSide = (side: LineRange['side']): 'LEFT' | 'RIGHT' => (side === 'deletions' ? 'LEFT' : 'RIGHT')

/**
 * GitHub's coordinates for a comment on `range`: `line`/`side` is the last line, `start_line`/`start_side` the first
 * (only for more than one line). A drag can go up; across sides the old (deleted) line comes first, as in the diff.
 */
export function toCommentTarget(path: string, range: LineRange): CommentTarget {
  let start = { line: range.start, side: githubSide(range.side) }
  let end = { line: range.end, side: githubSide(range.endSide ?? range.side) }
  const swap = start.side === end.side ? start.line > end.line : start.side === 'RIGHT'
  if (swap) [start, end] = [end, start]
  if (start.side === end.side && start.line === end.line) return { path, side: end.side, line: end.line }
  return { path, side: end.side, line: end.line, start_side: start.side, start_line: start.line }
}
