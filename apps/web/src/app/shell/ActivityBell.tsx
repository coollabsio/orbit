import { useState } from 'react'
import { Link, useLocation } from 'react-router'
import { ArrowRight, Bell, Notification as BellOutline } from 'reicon-react'
import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'
import { EmptyState } from '@/components/common/EmptyState'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from '@/components/ui/popover'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useLatestNotifications, useMarkAllNotificationsRead } from '@/features/inbox/api'
import { NotificationRow } from '@/features/inbox/components/NotificationRow'
import { useNotificationRows } from '@/features/inbox/useNotificationRows'
import { loadFailed } from '@/lib/connection'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { Shortcut } from '@/shortcuts/Shortcut'
import { useShortcutTitle } from '@/shortcuts/shortcutText'

/** How many notifications the popover shows; "View all activity" opens the rest. */
const LATEST = 10
/** The unread notifications the popover counts; more show as "99+". */
const UNREAD_PAGE = 100

type PanelTab = 'all' | 'unread'

/** Activity's address, with the project picked in Tasks (Activity opens filtered to it) and the tab. */
function useActivityPath(tab: PanelTab = 'all') {
  const { search } = useLocation()
  const params = new URLSearchParams()
  const project = new URLSearchParams(search).get('project')
  if (project) params.set('project', project)
  if (tab !== 'all') params.set('tab', tab)
  const query = params.toString()
  return query ? `/activity?${query}` : '/activity'
}

/**
 * Activity, beside the workspace name: it stays in view in every sidebar area, and a dot shows while something is
 * unread. In the sidebar it opens a popover with the newest notifications; on a phone's top bar it opens the page.
 */
export function ActivityBell({ unread = false, topbar = false }: { unread?: boolean; /** The phone top bar's look: page colours instead of the sidebar's. */ topbar?: boolean }) {
  const { pathname, key } = useLocation()
  const to = useActivityPath()
  // the popover belongs to the page it opened on: any navigation (a row, a shortcut, Back) closes it
  const [openOn, setOpenOn] = useState<string | null>(null)
  const open = openOn === key
  const setOpen = (next: boolean) => setOpenOn(next ? key : null)
  const onPage = pathname === '/activity' || pathname.startsWith('/activity/')
  const name = unread ? 'Activity, unread' : 'Activity'
  const tip = useShortcutTitle('nav.inbox', 'Activity')
  const className = cn(
    'relative flex shrink-0 items-center justify-center rounded-md transition-colors outline-none focus-visible:outline-1 focus-visible:outline-primary',
    topbar
      ? 'size-9 rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-muted data-[active=true]:text-foreground'
      : 'size-8 text-sidebar-nav hover:bg-black/4 hover:text-sidebar-nav-active data-[active=true]:bg-black/4 data-[active=true]:text-sidebar-nav-active dark:hover:bg-white/5 dark:data-[active=true]:bg-white/5',
  )
  const content = (
    <>
      <Bell className={topbar ? 'size-5' : 'size-[18px] opacity-90'} />
      {unread ? <span data-slot="activity-bell-dot" aria-hidden="true" className={cn('absolute size-2 rounded-full bg-primary ring-2', topbar ? 'top-2 right-2 ring-card' : 'top-1.5 right-1.5 ring-sidebar')} /> : null}
    </>
  )

  // a phone has room for the whole page, and no room for a popover beside it
  if (topbar) {
    return (
      <Tip label="Activity" side="bottom">
        <Link data-slot="activity-bell" data-active={onPage} aria-current={onPage ? 'page' : undefined} aria-label={name} to={to} className={className}>
          {content}
        </Link>
      </Tip>
    )
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tip label={open ? '' : tip} side="bottom">
        <PopoverTrigger
          data-slot="activity-bell"
          data-active={open || onPage}
          aria-current={onPage ? 'page' : undefined}
          aria-label={name}
          className={className}
        >
          {content}
        </PopoverTrigger>
      </Tip>
      <PopoverContent align="start" sideOffset={6} className="w-[min(380px,calc(100vw-24px))] gap-0 overflow-hidden p-0">
        <ActivityPanel onNavigate={() => setOpen(false)} />
      </PopoverContent>
    </Popover>
  )
}

/** The popover: the newest notifications or the unread ones, "Mark all read", and the way to the whole page. */
function ActivityPanel({ onNavigate }: { onNavigate: () => void }) {
  const { workspace } = useWorkspace()
  const [tab, setTab] = useState<PanelTab>('all')
  const latest = useLatestNotifications(workspace.id, { limit: LATEST })
  const unreadQuery = useLatestNotifications(workspace.id, { unread: true, limit: UNREAD_PAGE })
  const markAll = useMarkAllNotificationsRead(workspace.id)
  const marking = useSlowPending(markAll.isPending)
  // a row read a moment ago keeps its place in Unread until the list refreshes, without its dot
  const unread = unreadQuery.data?.items ?? []
  const unreadCount = unread.filter((notification) => !notification.read_at).length
  const countLabel = unreadQuery.data?.more ? `${UNREAD_PAGE - 1}+` : String(unreadCount)
  const query = tab === 'all' ? latest : unreadQuery
  const items = tab === 'all' ? (latest.data?.items ?? []) : unread.slice(0, LATEST)
  const rows = useNotificationRows([...(latest.data?.items ?? []), ...unread.slice(0, LATEST)])
  const pagePath = useActivityPath(tab)

  return (
    <>
      <div className="flex h-11 shrink-0 items-center gap-2 pr-2 pl-3">
        <PopoverTitle className="text-sm">Activity</PopoverTitle>
        {unreadCount > 0 ? <Badge className="h-4 min-w-4 px-1 text-[0.625rem] tabular-nums">{countLabel}</Badge> : null}
        <span className="flex-1" />
        <Button variant="ghost" size="xs" type="button" disabled={unreadCount === 0 || markAll.isPending} onClick={() => markAll.mutate()}>
          {marking ? 'Marking…' : 'Mark all read'}
        </Button>
      </div>
      <Tabs className="shrink-0 gap-0" value={tab} onValueChange={(value) => setTab(value as PanelTab)}>
        <TabsList variant="line" className="min-h-9 w-full justify-start gap-1 border-b px-2 group-data-horizontal/tabs:h-auto">
          <TabsTrigger value="all" className="h-7 flex-none px-2 text-[0.8125rem]">All</TabsTrigger>
          <TabsTrigger value="unread" className="h-7 flex-none gap-1.5 px-2 text-[0.8125rem]">
            Unread
            {unreadCount > 0 ? <span className="text-xs text-muted-foreground tabular-nums">{countLabel}</span> : null}
          </TabsTrigger>
        </TabsList>
      </Tabs>
      <div data-slot="activity-panel-list" className="max-h-[min(560px,calc(100dvh-180px))] min-h-0 overflow-y-auto overscroll-contain">
        {query.isPending ? (
          <div role="status" aria-label="Loading activity" className="flex flex-col">
            {[0, 1, 2].map((index) => (
              <div key={index} className="flex h-14 items-center gap-2.5 border-b px-3 last:border-b-0">
                <span className="w-2 shrink-0" />
                <Skeleton className="size-7 shrink-0 rounded-full" />
                <span className="flex flex-1 flex-col gap-1.5">
                  <Skeleton className="h-3 w-3/5" />
                  <Skeleton className="h-2.5 w-4/5" />
                </span>
              </div>
            ))}
          </div>
        ) : loadFailed(query) ? (
          <EmptyState
            size="sm"
            icon={BellOutline}
            title="Activity unavailable"
            description="Notifications could not be loaded."
            action={<Button variant="outline" size="sm" type="button" onClick={() => void query.refetch()}>Retry</Button>}
          />
        ) : items.length === 0 ? (
          tab === 'unread'
            ? <EmptyState size="sm" icon={BellOutline} title="No unread notifications" description="You're all caught up." />
            : <EmptyState size="sm" icon={BellOutline} title="You're all caught up" description="Assignments and mentions appear here." />
        ) : (
          // the last row's border would double the footer's
          <div className="[&>[data-slot=notification-row]:last-child_button]:border-b-0">
            {items.map((notification) => <NotificationRow key={notification.id} notification={notification} rows={rows} onOpen={onNavigate} />)}
          </div>
        )}
      </div>
      <Link
        to={pagePath}
        onClick={onNavigate}
        className="group/all flex h-10 shrink-0 items-center gap-1.5 border-t px-3 text-[0.8125rem] text-muted-foreground transition-colors outline-none hover:bg-muted hover:text-foreground focus-visible:bg-muted focus-visible:text-foreground"
      >
        View all activity
        <ArrowRight className="size-3.5 transition-transform duration-150 ease-out group-hover/all:translate-x-0.5 motion-reduce:transition-none" />
        <span className="flex-1" />
        <Shortcut id="nav.inbox" />
      </Link>
    </>
  )
}
