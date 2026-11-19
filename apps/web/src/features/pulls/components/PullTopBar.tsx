import { Dialog } from '@base-ui/react/dialog'
import { ArrowUpRight, Copy, SidebarRight, Xmark } from 'reicon-react'
import { toast } from 'sonner'
import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import type { PullRef } from '@orbit/api/github'
import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { MergeButton } from '@/features/pulls/components/MergeButton'
import { PullStateBadge } from '@/features/pulls/components/PullBits'
import { ReviewPanel } from '@/features/pulls/components/ReviewPanel'
import type { DiffStyle } from '@/features/pulls/useDiffStyle'
import { Shortcut } from '@/shortcuts/Shortcut'

async function copy(text: string, done: string) {
  try {
    await navigator.clipboard.writeText(text)
    toast.success(done)
  } catch {
    toast.error('Could not copy. Your browser did not allow it.')
  }
}

/** One quiet line: what the pull request is, and everything the reviewer acts with. Under it, the review progress. */
export function PullTopBar({ workspaceId, pullRef, pull, progress, diffStyle, onDiffStyle, activityOpen, onActivity, reviewOpen, onReviewOpen, onClose }: {
  workspaceId: string
  pullRef: PullRef
  pull: GithubPullDetail
  /** Share of the files to review that are viewed; `null` when there is nothing to review. */
  progress: number | null
  diffStyle: DiffStyle
  onDiffStyle: (style: DiffStyle) => void
  activityOpen: boolean
  onActivity: () => void
  reviewOpen: boolean
  onReviewOpen: (open: boolean) => void
  onClose: () => void
}) {
  return <header className="relative flex h-12 shrink-0 items-center gap-2.5 border-b pr-2 pl-3">
    <PullStateBadge state={pull.state} />
    <Dialog.Title className="min-w-0 truncate text-[15px] font-semibold text-foreground" title={pull.title}>{pull.title}</Dialog.Title>
    <span className="shrink-0 text-[15px] text-muted-foreground tabular-nums">#{pull.number}</span>
    <DropdownMenu>
      <DropdownMenuTrigger render={<button type="button" aria-label={`Branch ${pull.head_ref} into ${pull.base_ref}`} className="flex min-w-0 shrink items-center gap-1 rounded-md px-1.5 py-1 font-mono text-xs text-muted-foreground transition-colors duration-150 hover:bg-muted hover:text-foreground max-[1099px]:hidden" />}>
        <span className="min-w-0 max-w-52 truncate text-foreground">{pull.head_ref}</span>
        <span aria-hidden>→</span>
        <span className="shrink-0">{pull.base_ref}</span>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64">
        <DropdownMenuItem onClick={() => void copy(pull.head_ref, 'Branch name copied')}><Copy />Copy branch name</DropdownMenuItem>
        <DropdownMenuItem onClick={() => void copy(`gh pr checkout ${pull.number} --repo ${pull.repository}`, 'Checkout command copied')}><Copy />Copy checkout command</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
    <span className="shrink-0 font-mono text-xs tabular-nums max-[699px]:hidden">
      <span className="text-emerald-600 dark:text-emerald-400">+{pull.additions}</span> <span className="text-red-600 dark:text-red-400">−{pull.deletions}</span>
    </span>

    <div className="ml-auto flex shrink-0 items-center gap-1.5">
      <div role="radiogroup" aria-label="Diff layout" className="flex rounded-md bg-muted p-0.5 max-[799px]:hidden">
        {(['split', 'unified'] as const).map((style) => <button
          key={style}
          type="button"
          role="radio"
          aria-checked={diffStyle === style}
          onClick={() => onDiffStyle(style)}
          className={cn('h-6 rounded px-2 text-xs transition-colors duration-150', diffStyle === style ? 'bg-background text-foreground shadow-xs' : 'text-muted-foreground hover:text-foreground')}
        >{style === 'split' ? 'Split' : 'Unified'}</button>)}
      </div>
      <Tip label={<span className="flex items-center gap-2">{activityOpen ? 'Hide activity' : 'Show activity'}<Shortcut id="pull.activity" /></span>}>
        <Button variant="ghost" size="icon-sm" aria-label={activityOpen ? 'Hide activity' : 'Show activity'} aria-pressed={activityOpen} onClick={onActivity} className={cn(activityOpen && 'bg-muted text-foreground')}><SidebarRight /></Button>
      </Tip>
      <ReviewPanel workspaceId={workspaceId} pullRef={pullRef} viewer={pull.viewer} open={reviewOpen} onOpenChange={onReviewOpen} />
      <MergeButton workspaceId={workspaceId} pullRef={pullRef} pull={pull} />
      <Tip label="Open on GitHub">
        <a href={pull.html_url} target="_blank" rel="noopener noreferrer" aria-label="Open on GitHub" className="flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors duration-150 hover:bg-muted hover:text-foreground"><ArrowUpRight className="size-4" /></a>
      </Tip>
      <Tip label={<span className="flex items-center gap-2">Close<Shortcut id="detail.close" /></span>}>
        <Button variant="ghost" size="icon-sm" aria-label="Close" onClick={onClose}><Xmark /></Button>
      </Tip>
    </div>

    {progress !== null ? <div
      role="progressbar"
      aria-label="Files viewed"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(progress * 100)}
      className="absolute inset-x-0 -bottom-px h-0.5"
    >
      <div className={cn('pull-progress h-full', progress === 1 ? 'bg-emerald-500' : 'bg-primary')} style={{ width: `${progress * 100}%` }} />
    </div> : null}
  </header>
}
