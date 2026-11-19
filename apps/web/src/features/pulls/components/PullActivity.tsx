import { useLayoutEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { TickCircle } from 'reicon-react'
import type { GithubPullDetail, GithubPullTimelineItem, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { threadWaitsFor, type PullRef } from '@orbit/api/github'
import { cn } from 'cn'
import { GithubAvatar } from '@/features/pulls/components/PullBits'
import { ThreadView } from '@/features/pulls/components/ThreadView'
import { relativeTime } from '@/lib/format'
import { renderMarkdownBlocks } from '@/lib/markdown'

const VERB: Record<string, { text: string; className: string }> = {
  approved: { text: 'approved', className: 'text-emerald-700 dark:text-emerald-400' },
  changes_requested: { text: 'requested changes', className: 'text-red-600 dark:text-red-400' },
  commented: { text: 'reviewed', className: 'text-muted-foreground' },
}
const ago = (millis: number) => relativeTime(new Date(millis).toISOString())
const firstLine = (body: string) => body.replace(/[`*_>#[\]]/g, '').split('\n').find((line) => line.trim())?.trim() ?? ''

/** The right column: the description, the threads (open ones first, each a jump into the diff), what people said and
 *  decided, and the comments the diff cannot show. */
export function PullActivity({ workspaceId, pullRef, pull, threads, outside, timeline, loading, onJump }: {
  workspaceId: string
  pullRef: PullRef
  pull: GithubPullDetail
  threads: GithubReviewThread[]
  outside: GithubReviewThread[]
  timeline: GithubPullTimelineItem[]
  loading: boolean
  onJump: (thread: GithubReviewThread) => void
}) {
  const [showResolved, setShowResolved] = useState(false)
  const inDiff = threads.filter((thread) => !outside.includes(thread))
  const open = inDiff.filter((thread) => !thread.is_resolved)
  const resolved = inDiff.filter((thread) => thread.is_resolved)
  const waiting = (thread: GithubReviewThread) => threadWaitsFor(thread, pull.viewer.login, pull.author.login)
  return <aside aria-label="Activity" className="flex w-88 shrink-0 flex-col overflow-y-auto border-l max-[1099px]:w-80">
    <Description pull={pull} />
    {pull.tasks.length > 0 ? <Section title="Tasks">
      <div className="flex flex-col gap-0.5">{pull.tasks.map((task) => <Link key={task.task_id} to={`/tasks/${task.task_id}`} className="flex min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-muted">
        <TickCircle className="size-3.5 shrink-0 text-muted-foreground" /><span className="truncate">{task.title}</span>
      </Link>)}</div>
    </Section> : null}
    <Section title="Threads" count={open.length}>
      {open.length === 0 ? <p className="px-2 text-xs text-muted-foreground">{resolved.length > 0 ? 'Every thread is resolved.' : 'No comments on the code yet.'}</p> : null}
      <ul className="flex flex-col gap-0.5">{open.map((thread) => <ThreadRow key={thread.id} thread={thread} waiting={waiting(thread)} onJump={onJump} />)}</ul>
      {resolved.length > 0 ? <>
        <button type="button" className="mt-1 rounded-md px-2 py-1 text-left text-xs text-muted-foreground hover:bg-muted hover:text-foreground" onClick={() => setShowResolved(!showResolved)}>
          {showResolved ? 'Hide' : 'Show'} {resolved.length} resolved
        </button>
        {showResolved ? <ul className="flex flex-col gap-0.5 opacity-70">{resolved.map((thread) => <ThreadRow key={thread.id} thread={thread} waiting={false} onJump={onJump} />)}</ul> : null}
      </> : null}
    </Section>
    <Section title="Conversation">
      {loading ? <p className="px-2 text-xs text-muted-foreground">Loading…</p> : timeline.length === 0 ? <p className="px-2 text-xs text-muted-foreground">No reviews or comments yet.</p> : null}
      <ol className="flex flex-col gap-3">{timeline.map((item, index) => <li key={`${item.kind}-${item.created_at}-${index}`} className="flex gap-2 px-2 text-[13px]">
        <GithubAvatar actor={item.author} size={20} />
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-baseline gap-x-1.5 text-xs">
            <span className="font-medium text-foreground">{item.author.login}</span>
            <span className={VERB[item.review_state ?? '']?.className ?? 'text-muted-foreground'}>{item.review_state ? VERB[item.review_state]?.text ?? 'reviewed' : 'commented'}</span>
            <span className="text-muted-foreground">{ago(item.created_at)}</span>
          </p>
          {item.body.trim() ? <div className="mt-0.5 break-words text-foreground [&_p]:my-1">{renderMarkdownBlocks(item.body, `item-${index}`)}</div> : null}
        </div>
      </li>)}</ol>
    </Section>
    {outside.length > 0 ? <section id="pull-outside-diff" aria-label="Comments not in the diff" className="flex flex-col gap-2 border-t p-3">
      <h2 className="text-xs font-medium text-muted-foreground">Comments not in the diff</h2>
      {outside.map((thread) => <div key={thread.id} className="flex flex-col gap-1"><span className="font-mono text-[11px] text-muted-foreground">{thread.path}</span><ThreadView thread={thread} actions={{ workspaceId, pullRef }} /></div>)}
    </section> : null}
  </aside>
}

function Section({ title, count, children }: { title: string; count?: number; children: React.ReactNode }) {
  return <section aria-label={title} className="flex flex-col gap-1.5 border-t px-1.5 py-3 first:border-t-0">
    <h2 className="flex items-center gap-1.5 px-2 text-xs font-medium text-muted-foreground">{title}{count ? <span className="tabular-nums text-foreground">{count}</span> : null}</h2>
    {children}
  </section>
}

function ThreadRow({ thread, waiting, onJump }: { thread: GithubReviewThread; waiting: boolean; onJump: (thread: GithubReviewThread) => void }) {
  const first = thread.comments[0]
  const name = thread.path.slice(thread.path.lastIndexOf('/') + 1)
  return <li>
    <button type="button" onClick={() => onJump(thread)} className="flex w-full gap-2 rounded-md px-2 py-1.5 text-left transition-colors duration-150 hover:bg-muted">
      {first ? <GithubAvatar actor={first.author} size={18} /> : null}
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <span className="truncate font-mono" title={thread.path}>{name}{thread.line ? `:${thread.line}` : ''}</span>
          {thread.comments.length > 1 ? <span className="shrink-0 tabular-nums">{thread.comments.length} comments</span> : null}
          {waiting ? <span className="ml-auto shrink-0 rounded-full bg-primary px-1.5 text-[10px] font-medium text-primary-foreground">For you</span> : null}
        </span>
        <span className="line-clamp-2 text-xs text-foreground">{first ? firstLine(first.body) : ''}</span>
      </span>
    </button>
  </li>
}

/** The description, folded when it is long: the code is what the reviewer came for. */
function Description({ pull }: { pull: GithubPullDetail }) {
  const body = useRef<HTMLDivElement>(null)
  const [long, setLong] = useState(false)
  const [expanded, setExpanded] = useState(false)
  useLayoutEffect(() => { setLong((body.current?.scrollHeight ?? 0) > 220) }, [pull.body])
  return <section aria-label="Description" className="flex flex-col gap-1.5 px-3.5 py-3">
    <p className="flex items-center gap-1.5 text-xs text-muted-foreground"><GithubAvatar actor={pull.author} size={18} /><span className="font-medium text-foreground">{pull.author.login}</span>opened this</p>
    {pull.body.trim() ? <div
      ref={body}
      className={cn('break-words text-[13px] [&_[role=heading]]:mt-3! [&_[role=heading]]:text-[13px]! [&_[role=heading]]:leading-5! [&_[role=heading]]:font-semibold! [&_li]:my-0.5 [&_p]:my-1.5', long && !expanded && 'max-h-52 overflow-hidden [mask-image:linear-gradient(to_bottom,black_70%,transparent)]')}
    >{renderMarkdownBlocks(pull.body, 'pull-body')}</div> : <p className="text-[13px] text-muted-foreground">No description.</p>}
    {long ? <button type="button" className="self-start text-xs text-muted-foreground hover:text-foreground" onClick={() => setExpanded(!expanded)}>{expanded ? 'Show less' : 'Show all'}</button> : null}
  </section>
}
