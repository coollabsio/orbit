import { useSearchParams } from 'react-router'
import { Notification as Bell } from 'reicon-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { EmptyState } from '@/components/common/EmptyState'
import { VirtualList } from '@/components/common/VirtualList'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { isMention, useMarkAllNotificationsRead, useNotifications } from '@/features/inbox/api'
import { NotificationRow } from '@/features/inbox/components/NotificationRow'
import { useNotificationRows } from '@/features/inbox/useNotificationRows'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { loadFailed } from '@/lib/connection'

const TABS = ['all', 'unread', 'mentions'] as const
type InboxTab = (typeof TABS)[number]

export function InboxPage() {
  const { workspace } = useWorkspace()
  // the tab is in the URL: back from a notification's task, the same tab shows at the same scroll position
  const [searchParams, setSearchParams] = useSearchParams()
  const tab = TABS.find((item) => item === searchParams.get('tab')) ?? 'all'
  const setTab = (next: InboxTab) => setSearchParams((params) => {
    if (next === 'all') params.delete('tab')
    else params.set('tab', next)
    return params
  }, { replace: true })
  const allQuery = useNotifications(workspace.id, false)
  const unreadQuery = useNotifications(workspace.id, true)
  const rows = useNotificationRows(allQuery.data ?? [])
  const markAll = useMarkAllNotificationsRead(workspace.id)
  const marking = useSlowPending(markAll.isPending)
  const items = (allQuery.data ?? []).filter((notification) => {
    if (tab === 'unread') return !notification.read_at
    if (tab === 'mentions') return isMention(notification)
    return true
  })
  // a row read a moment ago stays in the unread list until it refreshes
  const unreadCount = (unreadQuery.data ?? allQuery.data ?? []).filter((notification) => !notification.read_at).length

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <PaneTitle>Activity</PaneTitle>
          {unreadCount > 0 ? <Badge className="h-4 min-w-4 px-1 text-[0.625rem] tabular-nums">{unreadCount}</Badge> : null}
          <span className="flex-1" />
          <Button
            variant="ghost"
            type="button"
            disabled={unreadCount === 0 || markAll.isPending}
            onClick={() => markAll.mutate()}
          >
            {marking ? 'Marking…' : 'Mark all read'}
          </Button>
        </PaneHeader>
        <Tabs className="shrink-0 gap-0" value={tab} onValueChange={(value) => setTab(value as InboxTab)}>
          <TabsList variant="line" className="min-h-10 w-full justify-start gap-1.5 border-b px-3 group-data-horizontal/tabs:h-auto">
            {TABS.map((item) => (
              <TabsTrigger
                key={item}
                value={item}
                className="h-7 flex-none px-2.5 text-[0.8125rem]"
              >
                {item === 'all' ? 'All' : item === 'unread' ? 'Unread' : 'Mentions'}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {allQuery.isPending || items.length === 0 ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            {allQuery.isPending ? (
              <EmptyState icon={Bell} title="Loading activity" description="Loading your notifications." />
            ) : loadFailed(allQuery) ? (
              <div className="flex h-full flex-col items-center justify-center">
                <EmptyState icon={Bell} title="Activity unavailable" description="Notifications could not be loaded." />
                <Button variant="outline" type="button" onClick={() => void allQuery.refetch()}>Retry</Button>
              </div>
            ) : (
              <EmptyState icon={Bell} title="You're all caught up" description="Assignments and mentions appear here." />
            )}
          </div>
        ) : (
          <VirtualList className="min-h-0 flex-1" count={items.length} rowHeight={56} rowKey={(index) => items[index].id}>
            {(index) => <NotificationRow notification={items[index]} rows={rows} />}
          </VirtualList>
        )}
      </Pane>
    </div>
  )
}
