import { useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { DirectInbox as Inbox, Menu } from 'reicon-react'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { relativeTime } from '@/lib/format'
import { EmptyState } from '@/components/common/EmptyState'
import { VirtualList } from '@/components/common/VirtualList'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useMembers } from '@/features/workspaces/api'
import { isMention, notificationCopy, notificationTarget, useMarkAllNotificationsRead, useMarkNotificationRead, useNotifications } from '@/features/inbox/api'
import { usePageTree } from '@/features/docs/api/pages'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'
import { loadFailed } from '@/lib/connection'

type InboxTab = 'all' | 'unread' | 'mentions'

export function InboxPage() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const location = useLocation()
  const [tab, setTab] = useState<InboxTab>('all')
  const allQuery = useNotifications(workspace.id, false)
  const unreadQuery = useNotifications(workspace.id, true)
  const members = useMembers(workspace.id)
  const hasPageMentions = (allQuery.data ?? []).some((notification) => notification.page_id)
  const pageTree = usePageTree(workspace.id, hasPageMentions)
  const markRead = useMarkNotificationRead(workspace.id)
  const markAll = useMarkAllNotificationsRead(workspace.id)
  const marking = useSlowPending(markAll.isPending)
  const items = (allQuery.data ?? []).filter((notification) => {
    if (tab === 'unread') return !notification.read_at
    if (tab === 'mentions') return isMention(notification)
    return true
  })
  const unreadCount = unreadQuery.data?.length ?? items.filter((notification) => !notification.read_at).length

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
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
          <PaneTitle>Inbox</PaneTitle>
          {unreadCount > 0 ? <Badge className="h-4 min-w-4 px-1 text-[10px] tabular-nums">{unreadCount}</Badge> : null}
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
            {(['all', 'unread', 'mentions'] as const).map((item) => (
              <TabsTrigger
                key={item}
                value={item}
                className="h-7 flex-none px-2.5 text-[13px]"
              >
                {item === 'all' ? 'All' : item === 'unread' ? 'Unread' : 'Mentions'}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {allQuery.isPending || items.length === 0 ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            {allQuery.isPending ? (
              <EmptyState icon={Inbox} title="Loading inbox" description="Loading your notifications." />
            ) : loadFailed(allQuery) ? (
              <div className="flex h-full flex-col items-center justify-center">
                <EmptyState icon={Inbox} title="Inbox unavailable" description="Notifications could not be loaded." />
                <Button variant="outline" type="button" onClick={() => void allQuery.refetch()}>Retry</Button>
              </div>
            ) : (
              <EmptyState icon={Inbox} title="You're all caught up" description="Assignments and mentions appear here." />
            )}
          </div>
        ) : (
          <VirtualList className="min-h-0 flex-1" count={items.length} rowHeight={56} rowKey={(index) => items[index].id}>
            {(index) => {
              const notification = items[index]
              const actor = members.data?.find((member) => member.id === notification.actor_user_id)
              const page = notification.page_id ? pageTree.data?.find((item) => item.id === notification.page_id) : undefined
              const copy = notificationCopy(notification, page?.title, actor?.name)
              return (
                // the actor's avatar is beside the row button, not inside it: a button cannot hold another one
                <div className="relative">
                <Button
                  variant="ghost"
                  className="h-auto min-h-14 w-full min-w-0 justify-start gap-2.5 rounded-none border-x-0 border-t-0 border-b-border px-3 py-1.5 text-left font-normal"
                  onClick={() => {
                    if (!notification.read_at) markRead.mutate(notification.id)
                    const target = notificationTarget(notification)
                    if (!target) return
                    if (notification.page_id || notification.chat_conversation_id) {
                      navigate(target)
                      return
                    }
                    const params = new URLSearchParams({ redirect: `${location.pathname}${location.search}` })
                    navigate(`${target}?${params}`)
                  }}
                >
                  <span className="flex w-2 shrink-0 justify-center">
                    {!notification.read_at ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
                  </span>
                  <span aria-hidden="true" className="size-7 shrink-0" />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[13px] text-foreground data-unread:font-semibold" data-unread={!notification.read_at || undefined}>
                      {copy.title}
                    </span>
                    <span className="truncate text-xs text-muted-foreground/70">{actor ? `${actor.name} · ${copy.body}` : copy.body}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground/70">
                    {relativeTime(notification.created_at)}
                  </span>
                </Button>
                {/* over the spacer: 12px padding, the 8px unread column and the 10px gap */}
                <ProfileTrigger userId={actor?.id} name={actor?.name ?? 'Unknown'} kind="avatar" className="absolute top-1/2 left-[30px] flex -translate-y-1/2">
                  <Avatar className="size-7">
                    <AvatarFallback>{(actor?.name ?? '?').charAt(0).toUpperCase()}</AvatarFallback>
                  </Avatar>
                </ProfileTrigger>
                </div>
              )
            }}
          </VirtualList>
        )}
      </Pane>
    </div>
  )
}
