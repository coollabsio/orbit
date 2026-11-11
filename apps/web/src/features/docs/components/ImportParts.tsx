import type { ReactNode } from 'react'
import { Link, useNavigate } from 'react-router'
import { ArrowLeft, DocumentText as FileText } from 'reicon-react'
import type { NotionImportProgress, NotionImportStatus, PageSummary } from '@orbit/api/generated/types.gen'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { PaneTitle } from '@/components/common/Pane'
import { relativeTime } from '@/lib/format'
import { isActiveImport } from '@/features/docs/api/notionImports'
import { pageTitle } from '@/features/docs/pageTree'

// Shared pieces of the Docs import panes (Notion, Obsidian).

export function ImportHeader({ title, detail, actions }: { title: string; detail?: string | null; actions?: ReactNode }) {
  const navigate = useNavigate()
  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="hidden text-muted-foreground/70 max-[899px]:inline-flex"
        aria-label="Back to docs"
        onClick={() => navigate('/docs')}
      >
        <ArrowLeft className="size-4" />
      </Button>
      <PaneTitle render={<h1 />}>{title}</PaneTitle>
      {detail ? <span className="truncate text-[0.8125rem] text-muted-foreground">{detail}</span> : null}
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

function importStatusVariant(status: NotionImportStatus): 'default' | 'secondary' | 'destructive' | 'outline' {
  if (status === 'failed') return 'destructive'
  if (status === 'completed') return 'secondary'
  if (isActiveImport(status) || status === 'ready') return 'default'
  return 'outline'
}

export function ProgressBar({ done, total, label }: { done: number; total: number; label: string }) {
  const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={done}
      className="h-2 w-full overflow-hidden rounded-full bg-muted"
    >
      <div className="h-full rounded-full bg-primary transition-[width] duration-500" style={{ width: `${percent}%` }} />
    </div>
  )
}

export function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex flex-col gap-0.5 rounded-lg border border-border px-3 py-2">
      <span className="text-lg font-semibold text-foreground tabular-nums">{value}</span>
      <span className="text-xs text-muted-foreground">{label}</span>
    </div>
  )
}


const IMPORT_STATUS_LABELS: Record<NotionImportStatus, string> = {
  scanning: 'Scanning',
  ready: 'Ready to import',
  queued: 'Queued',
  importing: 'Importing',
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
  expired: 'Expired',
}

interface ImportListItem {
  id: string
  status: NotionImportStatus
  progress: NotionImportProgress
  created_at: string
}

/** The member's recent imports, each linking to its pane. */
export function RecentImports<T extends ImportListItem>({
  imports,
  isPending,
  name,
  href,
}: {
  imports: T[] | undefined
  isPending: boolean
  name: (item: T) => string
  href: (item: T) => string
}) {
  if (isPending) return null
  if (!imports?.length) return null
  return (
    <section className="flex flex-col gap-2" aria-label="Recent imports">
      <h2 className="text-xs font-medium text-muted-foreground">Recent imports</h2>
      <div className="flex flex-col border-t border-border">
        {imports.map((item) => (
          <Link
            key={item.id}
            to={href(item)}
            className="flex min-h-10 min-w-0 items-center gap-3 border-b border-border px-1 py-1.5 text-[0.8125rem] transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
          >
            <span className="min-w-0 flex-1 truncate text-foreground">{name(item)}</span>
            {item.progress.total > 0 ? (
              <span className="shrink-0 text-xs text-muted-foreground">
                {item.progress.done} of {item.progress.total} pages
              </span>
            ) : null}
            <Badge variant={importStatusVariant(item.status)}>{IMPORT_STATUS_LABELS[item.status]}</Badge>
            <span className="w-12 shrink-0 text-right text-xs text-muted-foreground" title={new Date(item.created_at).toLocaleString()}>
              {relativeTime(item.created_at)}
            </span>
          </Link>
        ))}
      </div>
    </section>
  )
}

/** The imported top-level pages that still exist, with "Open imported pages". */
export function ImportedPages({ rootIds, pages }: { rootIds: string[]; pages: PageSummary[] | undefined }) {
  const navigate = useNavigate()
  if (rootIds.length === 0) return null
  const titles = new Map((pages ?? []).map((page) => [page.id, page]))
  return (
    <section className="flex flex-col gap-2" aria-label="Imported pages">
      <div className="flex items-center gap-2">
        <h3 className="text-xs font-medium text-muted-foreground">Imported pages</h3>
        <span className="flex-1" />
        <Button type="button" size="sm" onClick={() => navigate(`/docs/${rootIds[0]}`)}>
          Open imported pages
        </Button>
      </div>
      <ul className="flex flex-col border-t border-border">
        {rootIds.slice(0, 50).map((id) => (
          <li key={id} className="border-b border-border">
            <Link to={`/docs/${id}`} className="flex h-9 items-center gap-2 px-1 text-[0.8125rem] text-foreground transition-colors duration-150 ease-out hover:bg-sidebar-accent/50">
              <FileText className="size-[15px] shrink-0 text-muted-foreground/70" />
              <span className="truncate">{titles.has(id) ? pageTitle(titles.get(id)) : 'Imported page'}</span>
            </Link>
          </li>
        ))}
      </ul>
      {rootIds.length > 50 ? <p className="text-xs text-muted-foreground">and {rootIds.length - 50} more</p> : null}
    </section>
  )
}
