import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { ArrowUpRight, TickCircle } from 'reicon-react'
import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import { cn } from 'cn'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { GithubAvatar, PullStateBadge } from '@/features/pulls/components/PullBits'

const CHECK: Record<string, { label: string; dot: string }> = {
  success: { label: 'Checks passed', dot: 'bg-emerald-500' },
  failure: { label: 'Checks failed', dot: 'bg-red-500' },
  pending: { label: 'Checks running', dot: 'bg-amber-500' },
  neutral: { label: 'Checks done', dot: 'bg-muted-foreground' },
}

/** The pull request's title row: state, branches, author, checks and linked tasks. `actions` holds Review and Merge. */
export function PullHeader({ pull, actions }: { pull: GithubPullDetail; actions?: ReactNode }) {
  const check = CHECK[pull.checks.state]
  return <header className="flex flex-col gap-2 border-b px-4 py-3">
    <div className="flex items-start gap-3">
      <div className="flex min-w-0 flex-1 items-baseline gap-2">
        <h1 className="min-w-0 text-lg leading-snug font-semibold text-foreground">{pull.title}</h1>
        <span className="shrink-0 text-lg text-muted-foreground tabular-nums">#{pull.number}</span>
      </div>
      <div className="flex shrink-0 items-start gap-2">
        {actions}
        <a href={pull.html_url} target="_blank" rel="noopener noreferrer" aria-label="Open on GitHub" className="flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"><ArrowUpRight className="size-4" /></a>
      </div>
    </div>
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs text-muted-foreground">
      <PullStateBadge state={pull.state} />
      <span className="flex items-center gap-1.5"><GithubAvatar actor={pull.author} size={16} /><span className="text-foreground">{pull.author.login}</span> wants to merge into</span>
      <span className="flex items-center gap-1 font-mono"><Branch name={pull.base_ref} /><span aria-hidden>←</span><Branch name={pull.head_ref} /></span>
      <span className="tabular-nums"><span className="text-emerald-600 dark:text-emerald-400">+{pull.additions}</span> <span className="text-red-600 dark:text-red-400">−{pull.deletions}</span></span>
      {check ? <Popover>
        <PopoverTrigger className="flex items-center gap-1.5 rounded-md px-1 hover:bg-muted"><span className={cn('size-2 rounded-full', check.dot)} />{check.label}</PopoverTrigger>
        <PopoverContent className="w-72 p-1">
          <ul className="flex flex-col">{pull.checks.runs.map((run) => <li key={run.name}>
            <a href={run.url ?? pull.html_url} target="_blank" rel="noopener noreferrer" className="flex items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-muted">
              <span className={cn('size-2 shrink-0 rounded-full', CHECK[run.state]?.dot ?? 'bg-muted-foreground')} /><span className="truncate">{run.name}</span>
            </a>
          </li>)}</ul>
        </PopoverContent>
      </Popover> : null}
      {pull.tasks.map((task) => <Link key={task.task_id} to={`/tasks/${task.task_id}`} className="flex max-w-56 items-center gap-1 rounded-md border px-1.5 py-0.5 text-foreground hover:bg-muted">
        <TickCircle className="size-3 shrink-0 text-muted-foreground" /><span className="truncate">{task.title}</span>
      </Link>)}
    </div>
  </header>
}

function Branch({ name }: { name: string }) {
  return <span className="max-w-48 truncate rounded bg-muted px-1.5 py-0.5 text-foreground">{name}</span>
}
