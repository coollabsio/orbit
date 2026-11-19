import type { ReactNode } from 'react'
import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import type { ReviewSummary } from '@orbit/api/github'
import { cn } from 'cn'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

const DOT: Record<string, string> = { success: 'bg-emerald-500', failure: 'bg-red-500', pending: 'bg-amber-500', neutral: 'bg-muted-foreground' }
const row = 'flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left text-xs'

function Row({ label, children }: { label: string; children: ReactNode }) {
  return <><span className="text-muted-foreground">{label}</span><span className="flex min-w-0 items-center gap-1.5 text-foreground">{children}</span></>
}

const names = (logins: string[]) => logins.length <= 2 ? logins.join(' and ') : `${logins[0]} and ${logins.length - 1} more`

/** Where the pull request stands, in four short rows: what to read, who decided, what waits for the member, and CI. */
export function PullSummary({ pull, summary, onNextThread }: { pull: GithubPullDetail; summary: ReviewSummary; onNextThread: () => void }) {
  const checks = pull.checks
  const failed = checks.runs.filter((run) => run.state === 'failure').length
  const running = checks.runs.filter((run) => run.state === 'pending').length
  const checkText = checks.runs.length === 0 ? 'No checks'
    : failed > 0 ? `${failed} of ${checks.runs.length} failed`
      : running > 0 ? `${running} of ${checks.runs.length} running`
        : `${checks.runs.length} passed`
  return <section aria-label="Summary" className="grid shrink-0 grid-cols-[auto_1fr] gap-x-3 gap-y-0 border-b p-1.5 text-xs [&>*]:py-1.5 [&>*:nth-child(odd)]:pl-2">
    <Row label="Files">
      <span className="tabular-nums">{summary.viewed} of {summary.toReview} viewed</span>
      {summary.aside > 0 ? <span className="text-muted-foreground tabular-nums">{summary.aside} set aside</span> : null}
    </Row>
    <Row label="Reviews">
      {summary.approved.length === 0 && summary.changesRequested.length === 0 ? <span className="text-muted-foreground">None yet</span> : null}
      {summary.changesRequested.length > 0 ? <span className="truncate text-red-600 dark:text-red-400">Changes requested by {names(summary.changesRequested)}</span>
        : summary.approved.length > 0 ? <span className="truncate text-emerald-700 dark:text-emerald-400">Approved by {names(summary.approved)}</span> : null}
    </Row>
    <Row label="Threads">
      {summary.openThreads === 0 ? <span className="text-muted-foreground">{summary.resolvedThreads > 0 ? `All ${summary.resolvedThreads} resolved` : 'None'}</span> : <>
        <button type="button" className="tabular-nums underline-offset-2 hover:underline" onClick={onNextThread}>{summary.openThreads} open</button>
        {summary.needYou > 0 ? <span className="rounded-full bg-primary px-1.5 py-px text-[11px] font-medium text-primary-foreground tabular-nums">{summary.needYou} for you</span> : null}
      </>}
    </Row>
    <Row label="Checks">
      {checks.runs.length === 0 ? <span className="text-muted-foreground">{checkText}</span> : <Popover>
        <PopoverTrigger className="-mx-1 flex items-center gap-1.5 rounded px-1 hover:bg-muted">
          <span className={cn('size-2 rounded-full', DOT[checks.state] ?? DOT.neutral)} /><span className="tabular-nums">{checkText}</span>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-72 p-1">
          <ul className="flex flex-col">{checks.runs.map((run) => <li key={run.name}>
            <a href={run.url ?? pull.html_url} target="_blank" rel="noopener noreferrer" className={cn(row, 'hover:bg-muted')}>
              <span className={cn('size-2 shrink-0 rounded-full', DOT[run.state] ?? DOT.neutral)} /><span className="truncate">{run.name}</span>
            </a>
          </li>)}</ul>
        </PopoverContent>
      </Popover>}
    </Row>
  </section>
}
