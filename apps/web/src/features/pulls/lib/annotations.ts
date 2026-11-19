import type { DiffLineAnnotation } from '@pierre/diffs/react'
import type { GithubReviewThread } from '@orbit/api/generated/types.gen'

/** The review threads of one file as Pierre annotations: GitHub's `LEFT` is the old file (deletions). */
export function threadAnnotations(threads: GithubReviewThread[], path: string): DiffLineAnnotation<GithubReviewThread>[] {
  return threads
    .filter((thread) => thread.path === path && thread.line != null)
    .map((thread) => ({ side: thread.side === 'LEFT' ? 'deletions' : 'additions', lineNumber: thread.line as number, metadata: thread }))
}
