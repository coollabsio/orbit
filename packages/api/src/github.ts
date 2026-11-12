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

export type MergeMethod = 'merge' | 'squash' | 'rebase'
type MergeInfo = { methods: string[]; mergeable: string; blocked_reason?: string | null; default_method?: string | null }
type PullForMerge = { title: string; number: number; body: string; state: string; merge: MergeInfo; viewer: { can_merge: boolean } }

const isMethod = (value: string | null | undefined): value is MergeMethod => value === 'merge' || value === 'squash' || value === 'rebase'

/** Where the member's last merge method for a repository is kept. */
export function mergeMethodKey(ref: Pick<PullRef, 'owner' | 'repo'>): string {
  return `orbit:pulls:merge-method:${ref.owner}/${ref.repo}`
}

/** The merge method to offer: the stored one when the repository allows it, else its default, else the first allowed. */
export function pickMergeMethod(info: MergeInfo, stored: string | null): MergeMethod | null {
  const allowed = info.methods.filter(isMethod)
  if (isMethod(stored) && allowed.includes(stored)) return stored
  if (isMethod(info.default_method) && allowed.includes(info.default_method)) return info.default_method
  return allowed[0] ?? null
}

/** Why the member cannot merge now (shown next to the disabled button), or `null`. */
export function mergeDisabledReason(pull: PullForMerge): string | null {
  if (!pull.viewer.can_merge) return "You can't merge this pull request on GitHub."
  if (pull.merge.blocked_reason) return pull.merge.blocked_reason
  if (pull.merge.mergeable === 'conflicting') return 'This branch has conflicts.'
  if (!pull.merge.methods.some(isMethod)) return 'This repository allows no merge method.'
  return null
}

/** The squash commit GitHub would make: the title with the number, and the description as the message. */
export function squashDefaults(pull: Pick<PullForMerge, 'title' | 'number' | 'body'>): { title: string; message: string } {
  return { title: `${pull.title} (#${pull.number})`, message: pull.body }
}

const draftPrefix = (workspaceId: string, ref: PullRef) => `orbit:pulls:draft:${workspaceId}:${ref.owner}/${ref.repo}#${ref.number}`
const lineKey = (side: 'LEFT' | 'RIGHT', line: number) => `${side[0]}${line}`

/** Where a comment draft is kept: one key for each pull request, file and line range. */
export function draftKey(workspaceId: string, ref: PullRef, target: CommentTarget): string {
  const range = target.start_line != null && target.start_side ? `${lineKey(target.start_side, target.start_line)}-${lineKey(target.side, target.line)}` : lineKey(target.side, target.line)
  return `${draftPrefix(workspaceId, ref)}:${target.path}:${range}`
}

export function replyDraftKey(workspaceId: string, ref: PullRef, threadId: string): string {
  return `${draftPrefix(workspaceId, ref)}:reply:${threadId}`
}

/** The GitHub pull requests a text links to, in order, once each, at most `limit` (chat cards). */
export function extractPullRefs(text: string, limit = 3): PullRef[] {
  const refs: PullRef[] = []
  for (const match of text.matchAll(/https?:\/\/(?:www\.)?github\.com\/[^\s<>()[\]"']+/gi)) {
    // a link at the end of a sentence: the punctuation is not part of it
    const ref = parseGithubPullUrl(match[0].replace(/[.,;:!?]+$/, ''))
    if (!ref || refs.some((seen) => seen.owner === ref.owner && seen.repo === ref.repo && seen.number === ref.number)) continue
    refs.push(ref)
    if (refs.length === limit) break
  }
  return refs
}

/** Why a changed file is set aside from the review: machine-made files a reviewer rarely reads. `null` = review it. */
export type AsideReason = 'lockfile' | 'generated' | 'snapshot'

const LOCKFILES = new Set([
  'bun.lock', 'bun.lockb', 'package-lock.json', 'npm-shrinkwrap.json', 'yarn.lock', 'pnpm-lock.yaml', 'Cargo.lock',
  'Gemfile.lock', 'poetry.lock', 'uv.lock', 'Pipfile.lock', 'composer.lock', 'go.sum', 'flake.lock', 'Podfile.lock',
  'pubspec.lock', 'mix.lock', 'deno.lock',
])

export function asideReason(path: string): AsideReason | null {
  const name = path.slice(path.lastIndexOf('/') + 1)
  if (LOCKFILES.has(name)) return 'lockfile'
  if (/(^|\/)__snapshots__\//.test(path) || name.endsWith('.snap')) return 'snapshot'
  if (/(^|\/)(generated|__generated__)\//.test(path) || /\.(gen|generated|pb|min)\.[^.]+$/.test(name) || /_pb2\.py$/.test(name) || name.endsWith('.map')) return 'generated'
  return null
}

/** The parts of a pull request the review summary reads (the API types fit). */
type SummaryFile = { path: string; viewed: boolean }
type SummaryThread = { is_resolved: boolean; comments: { author: { login: string } }[] }
type SummaryItem = { kind: string; author: { login: string }; review_state?: string | null; created_at: number }

export type ReviewSummary = {
  /** Files to review (not set aside), and how many of them the member marked as viewed. */
  toReview: number
  viewed: number
  aside: number
  openThreads: number
  resolvedThreads: number
  /** Open threads waiting for the member: someone else wrote last, and the member wrote in it or owns the pull request. */
  needYou: number
  /** Each reviewer's latest decision (a later plain comment does not undo an approval, as on GitHub). */
  approved: string[]
  changesRequested: string[]
}

/** An open thread waits for the member when someone else wrote last, and the member wrote in it or owns the pull
 *  request. */
export function threadWaitsFor(thread: SummaryThread, viewer: string, author: string): boolean {
  const last = thread.comments.at(-1)?.author.login
  const involved = viewer === author || thread.comments.some((comment) => comment.author.login === viewer)
  return !thread.is_resolved && last !== undefined && last !== viewer && involved
}

export function reviewSummary({ files, threads, timeline, viewer, author }: {
  files: SummaryFile[]
  threads: SummaryThread[]
  timeline: SummaryItem[]
  /** The member's GitHub login, and the pull request author's. */
  viewer: string
  author: string
}): ReviewSummary {
  const review = files.filter((file) => asideReason(file.path) === null)
  const open = threads.filter((thread) => !thread.is_resolved)
  const decisions = new Map<string, string>()
  for (const item of [...timeline].sort((a, b) => a.created_at - b.created_at)) {
    if (item.kind === 'review' && (item.review_state === 'approved' || item.review_state === 'changes_requested')) decisions.set(item.author.login, item.review_state)
  }
  const who = (state: string) => [...decisions].filter(([, decision]) => decision === state).map(([login]) => login)
  return {
    toReview: review.length,
    viewed: review.filter((file) => file.viewed).length,
    aside: files.length - review.length,
    openThreads: open.length,
    resolvedThreads: threads.length - open.length,
    needYou: open.filter((thread) => threadWaitsFor(thread, viewer, author)).length,
    approved: who('approved'),
    changesRequested: who('changes_requested'),
  }
}

/** The `pr` search parameter that opens a pull request over the current page: `owner/repo/number`. */
export function pullParam(ref: PullRef): string {
  return `${ref.owner}/${ref.repo}/${ref.number}`
}

export function parsePullParam(value: string | null): PullRef | null {
  const match = /^([^/]+)\/([^/]+)\/(\d+)$/.exec(value ?? '')
  if (!match) return null
  const [, owner, repo, digits] = match
  const number = Number(digits)
  if (!NAME.test(owner) || !NAME.test(repo) || !Number.isSafeInteger(number) || number <= 0) return null
  return { owner, repo, number }
}
