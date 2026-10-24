import { useEffect, useRef, type ComponentType, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { MailLabel, MailThreadSummary } from '@/api/generated/types.gen'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { FOLDERS, mailViewPath, type MailView } from '@/features/mail/mailLib'
import { MailEmptyState } from './MailEmptyState'
import { ThreadRow } from './ThreadRow'

/** Rows left below the view when the next page loads. */
const LOAD_AHEAD = 5
const ROW_HEIGHT = 72

interface ThreadListProps {
  title: string
  view: MailView
  threads: MailThreadSummary[]
  labels: ReadonlyMap<string, MailLabel>
  status: 'pending' | 'error' | 'success'
  onRetry: () => void
  hasMore: boolean
  loadingMore: boolean
  onLoadMore: () => void
  activeThreadId: string | null
  focusedId: string | null
  selected: ReadonlySet<string>
  empty: { icon: ComponentType<{ className?: string }>; title: string; description: string; action?: ReactNode }
  /** The search box, in the header. */
  search: ReactNode
  /** Replaces the header while threads are selected. */
  bulkBar: ReactNode
  /** Over the list, e.g. a suspended mailbox. */
  notice?: ReactNode
  onOpen: (threadId: string) => void
  onSelect: (threadId: string) => void
  onStar: (thread: MailThreadSummary) => void
}

const MOBILE_VIEWS: Array<{ view: MailView; label: string }> = [
  { view: { kind: 'folder', folder: 'inbox' }, label: 'Inbox' },
  { view: { kind: 'starred' }, label: 'Starred' },
  ...FOLDERS.slice(1).map(({ folder, label }) => ({ view: { kind: 'folder', folder } as MailView, label })),
]

/** The middle pane: a virtual list of threads, newest first, that loads the next page near its end. */
export function ThreadList({
  title,
  view,
  threads,
  labels,
  status,
  onRetry,
  hasMore,
  loadingMore,
  onLoadMore,
  activeThreadId,
  focusedId,
  selected,
  empty,
  search,
  bulkBar,
  notice,
  onOpen,
  onSelect,
  onStar,
}: ThreadListProps) {
  const navigate = useNavigate()
  const scroller = useRef<HTMLDivElement>(null)
  const virtualizer = useVirtualizer({
    count: threads.length,
    getScrollElement: () => scroller.current,
    getItemKey: (index) => threads[index]?.id ?? index,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  })
  const items = virtualizer.getVirtualItems()
  const lastShown = items.at(-1)?.index ?? -1
  useEffect(() => {
    if (hasMore && !loadingMore && lastShown >= threads.length - LOAD_AHEAD) onLoadMore()
  }, [hasMore, loadingMore, lastShown, threads.length, onLoadMore])

  // j / k keep the focused row in view
  const focusedIndex = focusedId ? threads.findIndex((thread) => thread.id === focusedId) : -1
  useEffect(() => {
    if (focusedIndex >= 0) virtualizer.scrollToIndex(focusedIndex)
  }, [focusedIndex, virtualizer])

  const currentPath = mailViewPath(view)
  return (
    <section
      aria-label={title}
      className="flex h-full w-[360px] shrink-0 flex-col border-l bg-background max-[1199px]:w-[300px] max-[899px]:w-auto max-[899px]:flex-1 max-[899px]:border-l-0 max-[899px]:group-data-[view=thread]/mail:hidden"
    >
      {bulkBar ?? (
        <PaneHeader>
          <PaneTitle className="max-w-[40%] max-[899px]:hidden">{title}</PaneTitle>
          <PaneTitle className="hidden max-[899px]:block">Mail</PaneTitle>
          {search}
        </PaneHeader>
      )}
      <div className="hidden min-h-10 items-center gap-1.5 overflow-x-auto border-b px-3 py-1 [scrollbar-width:none] max-[899px]:flex">
        {MOBILE_VIEWS.map((entry) => (
          <Button
            key={entry.label}
            type="button"
            variant="ghost"
            size="sm"
            className="shrink-0 text-[13px] text-muted-foreground data-active:bg-primary/10 data-active:text-primary"
            data-active={mailViewPath(entry.view) === currentPath || undefined}
            onClick={() => navigate(mailViewPath(entry.view))}
          >
            {entry.label}
          </Button>
        ))}
      </div>
      {notice}
      {status === 'pending' ? (
        <div className="flex flex-col gap-3 p-3" aria-busy="true" aria-label="Loading conversations">
          {[0, 1, 2, 3, 4].map((index) => (
            <Skeleton key={index} className="h-14 w-full" />
          ))}
        </div>
      ) : status === 'error' && threads.length === 0 ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <MailEmptyState
            icon={empty.icon}
            title="Mail could not be loaded"
            description="Check your connection and try again."
            action={
              <Button variant="outline" size="sm" onClick={onRetry}>
                Try again
              </Button>
            }
          />
        </div>
      ) : threads.length === 0 ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <MailEmptyState icon={empty.icon} title={empty.title} description={empty.description} action={empty.action} />
        </div>
      ) : (
        <div ref={scroller} data-slot="mail-thread-list" className="min-h-0 flex-1 overflow-y-auto">
          <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
            {items.map((item) => {
              const thread = threads[item.index]
              return (
                <div key={item.key} ref={virtualizer.measureElement} data-index={item.index} className="absolute inset-x-0" style={{ top: item.start }}>
                  <ThreadRow
                    thread={thread}
                    labels={labels}
                    active={thread.id === activeThreadId}
                    focused={thread.id === focusedId}
                    selected={selected.has(thread.id)}
                    selecting={selected.size > 0}
                    onOpen={onOpen}
                    onSelect={onSelect}
                    onStar={onStar}
                  />
                </div>
              )
            })}
          </div>
          {loadingMore ? (
            <div className="flex justify-center py-3">
              <Spinner />
            </div>
          ) : null}
        </div>
      )}
    </section>
  )
}
