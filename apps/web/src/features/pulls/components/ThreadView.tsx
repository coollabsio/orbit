import type { GithubReviewThread } from '@orbit/api/generated/types.gen'
import { cn } from 'cn'
import { GithubAvatar } from '@/features/pulls/components/PullBits'
import { relativeTime } from '@/lib/format'
import { renderMarkdownBlocks } from '@/lib/markdown'

/** One review thread: its comments, and whether it is resolved, outdated or still pending. */
export function ThreadView({ thread, highlighted = false }: { thread: GithubReviewThread; highlighted?: boolean }) {
  return <div data-thread-id={thread.id} className={cn('my-1 flex flex-col gap-2 rounded-lg border bg-card px-3 py-2.5 font-sans text-[13px] text-foreground shadow-xs', highlighted && 'ring-2 ring-primary/50')}>
    {thread.is_resolved || thread.is_outdated ? <div className="flex gap-1.5">
      {thread.is_resolved ? <span className="rounded bg-muted px-1.5 text-[11px] text-muted-foreground">Resolved</span> : null}
      {thread.is_outdated ? <span className="rounded bg-muted px-1.5 text-[11px] text-muted-foreground">Outdated</span> : null}
    </div> : null}
    {thread.comments.map((comment) => <div key={comment.id} data-comment-id={comment.database_id} className="flex gap-2">
      <GithubAvatar actor={comment.author} size={22} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className="font-medium">{comment.author.login}</span>
          <span className="text-xs text-muted-foreground">{relativeTime(new Date(comment.created_at).toISOString())}</span>
          {comment.pending ? <span className="rounded bg-amber-500/15 px-1.5 text-[11px] text-amber-700 dark:text-amber-400">Pending</span> : null}
        </div>
        <div className="prose-sm mt-0.5 break-words [&_p]:my-1">{renderMarkdownBlocks(comment.body, comment.id)}</div>
      </div>
    </div>)}
  </div>
}
