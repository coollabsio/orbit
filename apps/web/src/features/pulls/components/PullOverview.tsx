import type { GithubPullDetail, GithubPullTimelineItem, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { GithubAvatar } from '@/features/pulls/components/PullBits'
import { ThreadView } from '@/features/pulls/components/ThreadView'
import { relativeTime } from '@/lib/format'
import { renderMarkdownBlocks } from '@/lib/markdown'

const REVIEW: Record<string, string> = { approved: 'approved', changes_requested: 'requested changes', commented: 'reviewed' }

/** The description, the conversation, and the comments the diff cannot show on a line. */
export function PullOverview({ pull, timeline, outside }: { pull: GithubPullDetail; timeline: GithubPullTimelineItem[]; outside: GithubReviewThread[] }) {
  return <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-4">
    <article className="rounded-lg border bg-card p-4 text-sm">
      <div className="mb-2 flex items-center gap-2 text-xs text-muted-foreground"><GithubAvatar actor={pull.author} size={18} /><span className="font-medium text-foreground">{pull.author.login}</span></div>
      {pull.body.trim() ? <div className="break-words [&_p]:my-1.5">{renderMarkdownBlocks(pull.body, 'pull-body')}</div> : <p className="text-muted-foreground">No description.</p>}
    </article>
    {timeline.map((item, index) => <article key={`${item.kind}-${item.created_at}-${index}`} className="rounded-lg border bg-card p-4 text-sm">
      <div className="mb-1.5 flex items-center gap-2 text-xs text-muted-foreground">
        <GithubAvatar actor={item.author} size={18} />
        <span className="font-medium text-foreground">{item.author.login}</span>
        {item.review_state ? <span>{REVIEW[item.review_state] ?? 'reviewed'}</span> : null}
        <span>{relativeTime(new Date(item.created_at).toISOString())}</span>
      </div>
      {item.body.trim() ? <div className="break-words [&_p]:my-1">{renderMarkdownBlocks(item.body, `item-${index}`)}</div> : null}
    </article>)}
    {outside.length > 0 ? <section aria-label="Comments not in the diff" className="flex flex-col gap-2">
      <h2 className="text-xs font-semibold text-muted-foreground">Comments not in the diff</h2>
      {outside.map((thread) => <div key={thread.id} className="flex flex-col gap-1"><span className="font-mono text-xs text-muted-foreground">{thread.path}</span><ThreadView thread={thread} /></div>)}
    </section> : null}
  </div>
}
