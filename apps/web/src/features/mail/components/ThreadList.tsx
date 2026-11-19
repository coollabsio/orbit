import { useEffect, useRef, type ComponentType, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { More, TickCircle } from 'reicon-react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { MailLabel, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { groupThreads, mailViewPath, railFolders, type MailView } from '@/features/mail/mailLib'
import { MailEmptyState } from './MailEmptyState'
import { ThreadRow } from './ThreadRow'
import { useMailAvatar } from './useMailAvatar'

/** Rows left below the view when the next page loads. */
const LOAD_AHEAD = 5
const ROW_HEIGHT = 72
const GROUP_HEIGHT = 30

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
  /** The threads with a draft of the user's. */
  draftThreads: ReadonlySet<string>
  /** The Outbox has something: its folder shows (§9.1 U11). */
  showOutbox: boolean
  /** Drafts, Scheduled and Outbox: their rows in place of threads, `count` of them. */
  rows?: { count: number; content: ReactNode }
  /** Unread conversations of the folder (from the counts); shown next to the title. */
  unreadCount?: number
  /** **All / Unread**; without it there is no switch (send folders, search). */
  unreadOnly?: { value: boolean; onChange: (unreadOnly: boolean) => void }
  /** Marks the unread conversations shown as read; without it the header has no menu. */
  onMarkAllRead?: () => void
  /** A thread's mailbox name when the list mixes mailboxes. */
  mailboxName?: (thread: MailThreadSummary) => string | undefined
  onOpen: (threadId: string) => void
  onSelect: (threadId: string) => void
  onStar: (thread: MailThreadSummary) => void
  onArchive: (thread: MailThreadSummary) => void
  onToggleRead: (thread: MailThreadSummary) => void
  onTrash: (thread: MailThreadSummary) => void
}

const mobileViews = (showOutbox: boolean): Array<{ view: MailView; label: string }> => [
  { view: { kind: 'folder', folder: 'inbox' }, label: 'Inbox' },
  { view: { kind: 'starred' }, label: 'Starred' },
  ...railFolders(showOutbox).map(({ folder, label }) => ({ view: { kind: 'folder', folder } as MailView, label })),
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
  draftThreads,
  showOutbox,
  rows,
  unreadCount = 0,
  unreadOnly,
  onMarkAllRead,
  mailboxName,
  onOpen,
  onSelect,
  onStar,
  onArchive,
  onToggleRead,
  onTrash,
}: ThreadListProps) {
  const navigate = useNavigate()
  const avatarOf = useMailAvatar()
  const scroller = useRef<HTMLDivElement>(null)
  const sentView = view.kind === 'folder' && view.folder === 'sent'
  // a header row before each date group (Today, Yesterday, …); search results keep their order without groups
  const entries = view.kind === 'search' ? threads.map((thread) => ({ kind: 'thread' as const, thread })) : groupThreads(threads, sentView)
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scroller.current,
    getItemKey: (index) => {
      const entry = entries[index]
      return !entry ? index : entry.kind === 'group' ? `group:${entry.label}` : entry.thread.id
    },
    estimateSize: (index) => (entries[index]?.kind === 'group' ? GROUP_HEIGHT : ROW_HEIGHT),
    overscan: 8,
  })
  const items = virtualizer.getVirtualItems()
  const lastShown = items.at(-1)?.index ?? -1
  useEffect(() => {
    if (hasMore && !loadingMore && lastShown >= entries.length - LOAD_AHEAD) onLoadMore()
  }, [hasMore, loadingMore, lastShown, entries.length, onLoadMore])

  // j / k keep the focused row in view
  const focusedIndex = focusedId ? entries.findIndex((entry) => entry.kind === 'thread' && entry.thread.id === focusedId) : -1
  useEffect(() => {
    if (focusedIndex >= 0) virtualizer.scrollToIndex(focusedIndex)
  }, [focusedIndex, virtualizer])

  const currentPath = mailViewPath(view)
  const count = rows ? rows.count : threads.length
  return (
    <section
      aria-label={title}
      className="flex h-full w-[360px] shrink-0 flex-col border-l bg-background max-[1199px]:w-[300px] max-[899px]:w-auto max-[899px]:flex-1 max-[899px]:border-l-0 max-[899px]:group-data-[view=thread]/mail:hidden"
    >
      {bulkBar ?? (
        <PaneHeader>
          <PaneTitle className="min-w-0 truncate max-[899px]:hidden">{title}</PaneTitle>
          <PaneTitle className="hidden max-[899px]:block">Mail</PaneTitle>
          {unreadCount > 0 ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{unreadCount > 999 ? '999+' : unreadCount} unread</span> : null}
          <span className="flex-1" />
          {unreadOnly ? (
            <ToggleGroup
              aria-label="Show"
              variant="outline"
              size="sm"
              value={[unreadOnly.value ? 'unread' : 'all']}
              onValueChange={(value: string[]) => { if (value[0]) unreadOnly.onChange(value[0] === 'unread') }}
            >
              <ToggleGroupItem value="all" className="h-7 px-2 text-xs font-normal text-muted-foreground aria-pressed:text-foreground">All</ToggleGroupItem>
              <ToggleGroupItem value="unread" className="h-7 px-2 text-xs font-normal text-muted-foreground aria-pressed:text-foreground">Unread</ToggleGroupItem>
            </ToggleGroup>
          ) : null}
          {onMarkAllRead ? (
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="List actions" />}>
                <More className="size-4" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-auto min-w-44">
                <DropdownMenuItem disabled={!threads.some((thread) => thread.unread)} onClick={onMarkAllRead}>
                  <TickCircle /> Mark all shown as read
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </PaneHeader>
      )}
      {bulkBar ? null : <div className="border-b px-3 py-2 [&_input]:w-full">{search}</div>}
      <div className="hidden min-h-10 items-center gap-1.5 overflow-x-auto border-b px-3 py-1 [scrollbar-width:none] max-[899px]:flex">
        {mobileViews(showOutbox).map((entry) => (
          <Button
            key={entry.label}
            type="button"
            variant="ghost"
            size="sm"
            className="shrink-0 text-[0.8125rem] text-muted-foreground data-active:bg-primary/10 data-active:text-primary"
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
      ) : status === 'error' && count === 0 ? (
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
      ) : count === 0 ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <MailEmptyState icon={empty.icon} title={empty.title} description={empty.description} action={empty.action} />
        </div>
      ) : rows ? (
        <div data-slot="mail-send-list" className="min-h-0 flex-1 overflow-y-auto">
          {rows.content}
        </div>
      ) : (
        <div ref={scroller} data-slot="mail-thread-list" className="min-h-0 flex-1 overflow-y-auto">
          <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
            {items.map((item) => {
              const entry = entries[item.index]
              return (
                <div key={item.key} ref={virtualizer.measureElement} data-index={item.index} className="absolute inset-x-0" style={{ top: item.start }}>
                  {entry.kind === 'group' ? (
                    <div data-slot="mail-date-group" className="flex h-[30px] items-end bg-background px-4 pb-1 text-[0.6875rem] font-medium tracking-wide text-muted-foreground uppercase">
                      {entry.label}
                    </div>
                  ) : (
                    <ThreadRow
                      thread={entry.thread}
                      labels={labels}
                      active={entry.thread.id === activeThreadId}
                      focused={entry.thread.id === focusedId}
                      selected={selected.has(entry.thread.id)}
                      selecting={selected.size > 0}
                      draft={draftThreads.has(entry.thread.id)}
                      sentView={sentView}
                      mailboxName={mailboxName?.(entry.thread)}
                      avatarOf={avatarOf}
                      onOpen={onOpen}
                      onSelect={onSelect}
                      onStar={onStar}
                      onArchive={onArchive}
                      onToggleRead={onToggleRead}
                      onTrash={onTrash}
                    />
                  )}
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
