import { Comment as CommentIcon, X } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'

export type CommentsFilter = 'open' | 'resolved'

/**
 * The page's "Comments" panel: Open / Resolved tabs over the thread list. The list itself (`ThreadList`, which needs
 * the editor) is portalled into `listRef` by the page editor.
 */
export function CommentsPanel({
  filter,
  onFilterChange,
  openCount,
  resolvedCount,
  loading,
  listRef,
  onClose,
}: {
  filter: CommentsFilter
  onFilterChange: (filter: CommentsFilter) => void
  openCount: number
  resolvedCount: number
  loading: boolean
  listRef: (element: HTMLDivElement | null) => void
  onClose: () => void
}) {
  const empty = filter === 'open' ? openCount === 0 : resolvedCount === 0
  return (
    <aside
      aria-label="Comments"
      data-comments-panel=""
      data-print-hide=""
      className="flex w-[340px] shrink-0 flex-col border-l border-border bg-background max-[1099px]:absolute max-[1099px]:inset-y-0 max-[1099px]:right-0 max-[1099px]:z-20 max-[1099px]:shadow-xl max-[599px]:w-full"
    >
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <span className="text-[13px] font-semibold text-foreground">Comments</span>
        <span className="flex-1" />
        <Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label="Close comments" onClick={onClose}>
          <X className="size-4" />
        </Button>
      </div>
      <Tabs className="shrink-0 gap-0 px-2 pt-2" value={filter} onValueChange={(value) => onFilterChange(value as CommentsFilter)}>
        <TabsList className="w-full">
          {(['open', 'resolved'] as const).map((item) => {
            const count = item === 'open' ? openCount : resolvedCount
            return (
              <TabsTrigger key={item} value={item} className="gap-1.5 text-[13px]">
                {item === 'open' ? 'Open' : 'Resolved'}
                <span
                  className={cn(
                    'min-w-[18px] rounded-full px-1 text-center text-[11px] leading-4 font-semibold tabular-nums',
                    item === 'open' && count > 0 && filter === 'open'
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-foreground/8 text-muted-foreground dark:bg-foreground/10',
                  )}
                >
                  {count}
                </span>
              </TabsTrigger>
            )
          })}
        </TabsList>
      </Tabs>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-2">
        {empty ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 pb-16 text-center" data-comments-empty={filter}>
            <span className="flex size-9 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <CommentIcon className="size-[18px]" aria-hidden="true" />
            </span>
            {loading ? (
              <p className="text-[13px] text-muted-foreground">Loading comments…</p>
            ) : filter === 'open' ? (
              <>
                <p className="text-[13px] font-medium text-foreground">No open comments</p>
                <p className="max-w-[260px] text-xs leading-[18px] text-muted-foreground">Select text and click Comment to start a thread.</p>
              </>
            ) : (
              <>
                <p className="text-[13px] font-medium text-foreground">No resolved comments</p>
                <p className="max-w-[240px] text-xs leading-[18px] text-muted-foreground">Resolved threads show up here.</p>
              </>
            )}
          </div>
        ) : null}
        <div ref={listRef} hidden={empty} />
      </div>
    </aside>
  )
}
