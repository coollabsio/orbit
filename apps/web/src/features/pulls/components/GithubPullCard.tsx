import { cn } from 'cn'
import type { PullRef } from '@orbit/api/github'
import { usePull } from '@/features/pulls/api'
import { GithubAvatar, PullStateBadge } from '@/features/pulls/components/PullBits'
import { PullLink } from '@/features/pulls/components/PullLink'

const CHECK_DOT: Record<string, string> = { success: 'bg-emerald-500', failure: 'bg-red-500', pending: 'bg-amber-500' }

/** A GitHub pull request link in chat, as a card that opens Orbit's viewer. Only a reader whose own GitHub account
 *  can see the pull request gets the card; everyone else sees nothing here (the link stays in the text). */
export function GithubPullCard({ workspaceId, pullRef }: { workspaceId: string; pullRef: PullRef }) {
  const pull = usePull(workspaceId, pullRef)
  if (!pull.data) return null
  const data = pull.data
  return <PullLink pullRef={pullRef} className="flex min-w-0 flex-col gap-1 rounded-lg border bg-card px-3 py-2 text-[13px] transition-colors duration-150 ease-out hover:bg-muted/60">
    <span className="flex min-w-0 items-center gap-2">
      <PullStateBadge state={data.state} />
      <span className="min-w-0 truncate font-medium text-foreground">{data.title}</span>
    </span>
    <span className="flex items-center gap-2 text-xs text-muted-foreground">
      <GithubAvatar actor={data.author} size={14} />
      <span className="truncate">{data.repository}#{data.number}</span>
      <span className="tabular-nums"><span className="text-emerald-600 dark:text-emerald-400">+{data.additions}</span> <span className="text-red-600 dark:text-red-400">−{data.deletions}</span></span>
      {CHECK_DOT[data.checks.state] ? <span className={cn('size-2 rounded-full', CHECK_DOT[data.checks.state])} aria-label={`Checks ${data.checks.state}`} /> : null}
    </span>
  </PullLink>
}
