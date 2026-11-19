import { cn } from 'cn'
import type { GithubActor } from '@orbit/api/generated/types.gen'

const STATE: Record<string, { label: string; className: string }> = {
  open: { label: 'Open', className: 'bg-emerald-500/12 text-emerald-700 dark:text-emerald-400' },
  draft: { label: 'Draft', className: 'bg-muted text-muted-foreground' },
  merged: { label: 'Merged', className: 'bg-violet-500/12 text-violet-700 dark:text-violet-400' },
  closed: { label: 'Closed', className: 'bg-red-500/12 text-red-700 dark:text-red-400' },
}

/** GitHub's state of a pull request as a small pill: open, draft, merged or closed. */
export function PullStateBadge({ state, className }: { state: string; className?: string }) {
  const shown = STATE[state] ?? STATE.open
  return <span className={cn('inline-flex h-5 shrink-0 items-center rounded-full px-2 text-[11px] font-medium', shown.className, className)}>{shown.label}</span>
}

/** A GitHub account's picture (GitHub serves it; `img-src` allows https). */
export function GithubAvatar({ actor, size = 20 }: { actor: GithubActor; size?: number }) {
  return actor.avatar_url
    ? <img src={actor.avatar_url} alt="" width={size} height={size} className="shrink-0 rounded-full bg-muted" style={{ width: size, height: size }} loading="lazy" />
    : <span aria-hidden className="shrink-0 rounded-full bg-muted" style={{ width: size, height: size }} />
}
