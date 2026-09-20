import { useState } from 'react'
import { useNavigate } from 'react-router'
import { DirectInbox as Inbox, Menu } from 'reicon-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { relativeTime } from '@/lib/format'
import { EmptyState } from '@/components/common/EmptyState'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useMembers } from '@/features/workspaces/api'
import { notificationCopy, useMarkAllNotificationsRead, useMarkNotificationRead, useNotifications } from '@/features/inbox/api'

type InboxTab = 'all' | 'unread' | 'mentions'

export function InboxPage() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const [tab, setTab] = useState<InboxTab>('all')
  const allQuery = useNotifications(workspace.id, false)
  const unreadQuery = useNotifications(workspace.id, true)
  const members = useMembers(workspace.id)
  const markRead = useMarkNotificationRead(workspace.id)
  const markAll = useMarkAllNotificationsRead(workspace.id)
  const items = (allQuery.data ?? []).filter((notification) => {
    if (tab === 'unread') return !notification.read_at
    if (tab === 'mentions') return notification.kind === 'comment_mentioned'
    return true
  })
  const unreadCount = unreadQuery.data?.length ?? items.filter((notification) => !notification.read_at).length

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
        <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2 max-[899px]:border-b-0">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="hidden shrink-0 text-muted-foreground/70 max-[899px]:inline-flex"
            aria-label="Menu"
            onClick={() => window.dispatchEvent(new CustomEvent('open-sidebar'))}
          >
            <Menu className="size-[18px]" />
          </Button>
          <span className="truncate text-[13px] font-semibold text-foreground">Inbox</span>
          {unreadCount > 0 ? <Badge className="h-4 min-w-4 rounded-full border-0 px-1 py-0 text-[10px] font-semibold">{unreadCount}</Badge> : null}
          <span className="flex-1" />
          <Button
            variant="ghost"
            type="button"
            disabled={unreadCount === 0 || markAll.isPending}
            onClick={() => markAll.mutate()}
          >
            {markAll.isPending ? 'Marking…' : 'Mark all read'}
          </Button>
        </div>
        <Tabs className="shrink-0 gap-0" value={tab} onValueChange={(value) => setTab(value as InboxTab)}>
          <TabsList variant="line" className="min-h-10 w-full justify-start gap-1.5 overflow-x-auto rounded-none border-b border-border px-3 py-1 [scrollbar-width:none] group-data-horizontal/tabs:h-auto">
            {(['all', 'unread', 'mentions'] as const).map((item) => (
              <TabsTrigger
                key={item}
                value={item}
                className="h-7 flex-none gap-1 rounded-md border-0 px-2.5 py-0 text-[13px] font-medium text-muted-foreground after:hidden hover:bg-accent hover:text-foreground dark:text-muted-foreground data-active:bg-primary/10! data-active:text-primary! data-active:ring-1 data-active:ring-primary/25 data-active:ring-inset"
              >
                {item === 'all' ? 'All' : item === 'unread' ? 'Unread' : 'Mentions'}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {allQuery.isPending ? (
            <EmptyState icon={Inbox} title="Loading inbox" description="Loading your notifications." />
          ) : allQuery.isError ? (
            <div className="flex h-full flex-col items-center justify-center">
              <EmptyState icon={Inbox} title="Inbox unavailable" description="Notifications could not be loaded." />
              <Button variant="outline" type="button" onClick={() => void allQuery.refetch()}>Retry</Button>
            </div>
          ) : items.length === 0 ? (
            <EmptyState icon={Inbox} title="You're all caught up" description="Assignments and mentions appear here." />
          ) : (
            items.map((notification) => {
              const actor = members.data?.find((member) => member.id === notification.actor_user_id)
              const copy = notificationCopy(notification)
              return (
                <Button
                  key={notification.id}
                  variant="ghost"
                  className="flex h-auto min-h-14 w-full min-w-0 cursor-pointer justify-start gap-2.5 rounded-none border-0 border-b border-border px-3 py-1.5 text-left font-normal hover:bg-foreground/[0.02] active:not-aria-[haspopup]:translate-y-0 dark:hover:bg-foreground/[0.02]"
                  onClick={() => {
                    if (!notification.read_at) markRead.mutate(notification.id)
                    navigate(`/tasks/${notification.task_id}`)
                  }}
                >
                  <span className="flex w-2 shrink-0 justify-center">
                    {!notification.read_at ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
                  </span>
                  <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">{(actor?.name ?? '?').charAt(0).toUpperCase()}</span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[13px] text-foreground data-[unread]:font-semibold" data-unread={!notification.read_at || undefined}>
                      {copy.title}
                    </span>
                    <span className="truncate text-xs text-muted-foreground/70">{actor ? `${actor.name} · ${copy.body}` : copy.body}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground/70">
                    {relativeTime(notification.created_at)}
                  </span>
                </Button>
              )
            })
          )}
        </div>
      </div>
    </div>
  )
}
