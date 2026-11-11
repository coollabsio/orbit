import type { ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { ArrowLeft } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { PaneTitle } from '@/components/common/Pane'

/** The 48px title row of a task import pane, with a back button on narrow screens. */
export function ImportHeader({ title, detail, actions }: { title: string; detail?: string | null; actions?: ReactNode }) {
  const navigate = useNavigate()
  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="hidden text-muted-foreground/70 max-[899px]:inline-flex"
        aria-label="Back to tasks"
        onClick={() => navigate('/tasks')}
      >
        <ArrowLeft className="size-4" />
      </Button>
      <PaneTitle render={<h1 />}>{title}</PaneTitle>
      {detail ? <span className="truncate text-[13px] text-muted-foreground">{detail}</span> : null}
      <span className="flex-1" />
      {actions}
    </div>
  )
}

export function ImportBody({ children }: { children: ReactNode }) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 p-6 max-[899px]:p-4">{children}</div>
    </div>
  )
}

export function ImportProgressBar({ done, total }: { done: number; total: number }) {
  const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0
  return (
    <div
      role="progressbar"
      aria-label="Import progress"
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={done}
      className="h-2 w-full overflow-hidden rounded-full bg-muted"
    >
      <div className="h-full rounded-full bg-primary transition-[width] duration-500" style={{ width: `${percent}%` }} />
    </div>
  )
}

export function ImportStat({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex flex-col gap-0.5 rounded-lg border border-border px-3 py-2">
      <span className="text-lg font-semibold text-foreground tabular-nums">{value}</span>
      <span className="text-xs text-muted-foreground">{label}</span>
    </div>
  )
}
