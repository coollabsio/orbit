import { useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { AlarmSleep, Archive, ArchiveUp, Eye, EyeSlash, Menu, Notification as Bell, Undo } from 'reicon-react'
import type { NotificationPatchBody, NotificationRecord } from '@/api/generated/types.gen'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { relativeTime } from '@/lib/format'
import { EmptyState } from '@/components/common/EmptyState'
import { VirtualList } from '@/components/common/VirtualList'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useMembers } from '@/features/workspaces/api'
import { isMention, notificationCopy, notificationTarget, snoozePresets, useMarkAllNotificationsRead, useNotifications, usePatchNotification } from '@/features/inbox/api'
import { usePageTree } from '@/features/docs/api/pages'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'
import { loadFailed } from '@/lib/connection'
import { useCommand } from '@/shortcuts/useCommand'

const TABS = [
  { value: 'all', label: 'All', empty: 'Assignments, mentions and updates of tasks you follow appear here.' },
  { value: 'unread', label: 'Unread', empty: 'Nothing is unread.' },
  { value: 'mentions', label: 'Mentions', empty: 'Nobody mentioned you.' },
  { value: 'snoozed', label: 'Snoozed', empty: 'A snoozed notification waits here until its time, or until something new happens.' },
  { value: 'archived', label: 'Archived', empty: 'Archived notifications appear here.' },
] as const
type InboxTab = (typeof TABS)[number]['value']

/** `2026-10-08T15:30`, the value of a `datetime-local` input, in local time. */
function localInputValue(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}

export function InboxPage() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const location = useLocation()
  const [tab, setTab] = useState<InboxTab>('all')
  const state = tab === 'snoozed' || tab === 'archived' ? tab : 'inbox'
  const query = useNotifications(workspace.id, false, state)
  const unreadQuery = useNotifications(workspace.id, true)
  const members = useMembers(workspace.id)
  const hasPageMentions = (query.data ?? []).some((notification) => notification.page_id)
  const pageTree = usePageTree(workspace.id, hasPageMentions)
  const patch = usePatchNotification(workspace.id)
  const markAll = useMarkAllNotificationsRead(workspace.id)
  const marking = useSlowPending(markAll.isPending)
  const items = (query.data ?? []).filter((notification) => {
    if (tab === 'unread') return !notification.read_at
    if (tab === 'mentions') return isMention(notification)
    return true
  })
  const unreadCount = unreadQuery.data?.length ?? 0
  // the row under the pointer or with the focus: the one the shortcuts act on
  const active = useRef<NotificationRecord | null>(null)
  const [snoozeMenu, setSnoozeMenu] = useState<string | null>(null)
  const [custom, setCustom] = useState<{ id: string; value: string } | null>(null)

  /** Changes a notification; `undo` puts a toast with the way back. */
  const change = (id: string, body: NotificationPatchBody, undo?: { message: string; body: NotificationPatchBody }) =>
    patch.mutate({ id, body }, {
      onSuccess: () => {
        if (undo) toast(undo.message, { action: { label: 'Undo', onClick: () => patch.mutate({ id, body: undo.body }) } })
      },
      onError: () => toast.error('Could not change the notification. Try again.'),
    })
  const archive = (id: string) => change(id, { archived: true }, { message: 'Archived', body: { archived: false } })
  const snooze = (id: string, until: Date) =>
    change(id, { snoozed_until: until.toISOString() }, { message: 'Snoozed', body: { snoozed_until: null } })
  const toggleRead = (notification: NotificationRecord) => change(notification.id, { read: !notification.read_at })

  const inInbox = state === 'inbox'
  const target = () => (inInbox && items.some((item) => item.id === active.current?.id) ? active.current : null)
  const available = () => target() !== null
  useCommand('inbox.archive', () => { const row = target(); if (row) archive(row.id) }, { available })
  useCommand('inbox.snooze', () => { const row = target(); if (row) setSnoozeMenu(row.id) }, { available })
  useCommand('inbox.toggleRead', () => { const row = target(); if (row) toggleRead(row) }, { available })

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
          <PaneTitle>Activity</PaneTitle>
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
          <TabsList variant="line" className="min-h-10 w-full justify-start gap-1.5 overflow-x-auto border-b px-3 group-data-horizontal/tabs:h-auto">
            {TABS.map((item) => (
              <TabsTrigger key={item.value} value={item.value} className="h-7 flex-none px-2.5 text-[13px]">
                {item.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {query.isPending || items.length === 0 ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            {query.isPending ? (
              <EmptyState icon={Bell} title="Loading activity" description="Loading your notifications." />
            ) : loadFailed(query) ? (
              <div className="flex h-full flex-col items-center justify-center">
                <EmptyState icon={Bell} title="Activity unavailable" description="Notifications could not be loaded." />
                <Button variant="outline" type="button" onClick={() => void query.refetch()}>Retry</Button>
              </div>
            ) : (
              <EmptyState
                icon={Bell}
                title={tab === 'all' ? "You're all caught up" : `No ${TABS.find((item) => item.value === tab)!.label.toLowerCase()} notifications`}
                description={TABS.find((item) => item.value === tab)!.empty}
              />
            )}
          </div>
        ) : (
          <VirtualList className="min-h-0 flex-1" count={items.length} rowHeight={56} rowKey={(index) => items[index].id}>
            {(index) => {
              const notification = items[index]
              const actor = members.data?.find((member) => member.id === notification.actor_user_id)
              const page = notification.page_id ? pageTree.data?.find((item) => item.id === notification.page_id) : undefined
              const copy = notificationCopy(notification, page?.title, actor?.name)
              const unread = !notification.read_at
              return (
                // the actor's avatar and the actions are beside the row button, not inside it: a button cannot hold another one
                <div
                  className="group/row relative"
                  onMouseEnter={() => { active.current = notification }}
                  onMouseLeave={() => { if (active.current?.id === notification.id) active.current = null }}
                  onFocus={() => { active.current = notification }}
                >
                <Button
                  variant="ghost"
                  className="h-auto min-h-14 w-full min-w-0 justify-start gap-2.5 rounded-none border-x-0 border-t-0 border-b-border px-3 py-1.5 text-left font-normal"
                  onClick={() => {
                    if (unread) patch.mutate({ id: notification.id, body: { read: true } })
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
                    {unread ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
                  </span>
                  <span aria-hidden="true" className="size-7 shrink-0" />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[13px] text-foreground data-unread:font-semibold" data-unread={unread || undefined}>
                      {copy.title}
                    </span>
                    <span className="truncate text-xs text-muted-foreground/70">{copy.body}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground/70 group-focus-within/row:invisible group-hover/row:invisible data-menu:invisible" data-menu={snoozeMenu === notification.id || undefined}>
                    {state === 'snoozed' && notification.snoozed_until
                      ? `Until ${new Date(notification.snoozed_until).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })}`
                      : relativeTime(notification.created_at)}
                  </span>
                </Button>
                {/* over the spacer: 12px padding, the 8px unread column and the 10px gap */}
                <ProfileTrigger userId={actor?.id} name={actor?.name ?? 'Unknown'} kind="avatar" className="absolute top-1/2 left-[30px] flex -translate-y-1/2">
                  <Avatar className="size-7">
                    <AvatarFallback>{(actor?.name ?? (notification.task_id && !notification.actor_user_id ? 'G' : '?')).charAt(0).toUpperCase()}</AvatarFallback>
                  </Avatar>
                </ProfileTrigger>
                {/* over the time; shown for the row under the pointer or with the focus, and while its menu is open */}
                <div
                  className="absolute top-1/2 right-3 hidden -translate-y-1/2 items-center gap-0.5 group-focus-within/row:flex group-hover/row:flex data-menu:flex"
                  data-menu={snoozeMenu === notification.id || undefined}
                >
                  {state === 'snoozed' ? (
                    <Button type="button" variant="ghost" size="icon-sm" aria-label="Unsnooze" title="Unsnooze" onClick={() => change(notification.id, { snoozed_until: null })}>
                      <Undo />
                    </Button>
                  ) : state === 'archived' ? (
                    <Button type="button" variant="ghost" size="icon-sm" aria-label="Move to inbox" title="Move to inbox" onClick={() => change(notification.id, { archived: false })}>
                      <ArchiveUp />
                    </Button>
                  ) : (
                    <>
                      <DropdownMenu open={snoozeMenu === notification.id} onOpenChange={(open) => setSnoozeMenu(open ? notification.id : null)}>
                        <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" aria-label="Snooze" title="Snooze"><AlarmSleep /></Button>} />
                        <DropdownMenuContent align="end" className="w-auto min-w-52">
                          {snoozePresets(new Date()).map((preset) => (
                            <DropdownMenuItem key={preset.label} onClick={() => snooze(notification.id, preset.until)}>
                              {preset.label}
                              <span className="ml-auto pl-4 text-xs text-muted-foreground">
                                {preset.until.toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' })}
                              </span>
                            </DropdownMenuItem>
                          ))}
                          <DropdownMenuSeparator />
                          <DropdownMenuItem onClick={() => setCustom({ id: notification.id, value: localInputValue(snoozePresets(new Date())[1].until) })}>
                            Custom date and time…
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                      <Button type="button" variant="ghost" size="icon-sm" aria-label="Archive" title="Archive" onClick={() => archive(notification.id)}>
                        <Archive />
                      </Button>
                      <Button type="button" variant="ghost" size="icon-sm" aria-label={unread ? 'Mark read' : 'Mark unread'} title={unread ? 'Mark read' : 'Mark unread'} onClick={() => toggleRead(notification)}>
                        {unread ? <Eye /> : <EyeSlash />}
                      </Button>
                    </>
                  )}
                </div>
                </div>
              )
            }}
          </VirtualList>
        )}
      </Pane>
      <Dialog open={custom !== null} onOpenChange={(open) => { if (!open) setCustom(null) }}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Snooze until</DialogTitle>
            <DialogDescription>The notification comes back at this time, or earlier when something new happens.</DialogDescription>
          </DialogHeader>
          <form
            className="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault()
              if (!custom) return
              const until = new Date(custom.value)
              if (Number.isNaN(until.getTime()) || until <= new Date()) {
                toast.error('Choose a time in the future.')
                return
              }
              snooze(custom.id, until)
              setCustom(null)
            }}
          >
            <Input
              type="datetime-local"
              aria-label="Snooze until"
              required
              min={localInputValue(new Date())}
              value={custom?.value ?? ''}
              onChange={(event) => setCustom((current) => (current ? { ...current, value: event.target.value } : current))}
            />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setCustom(null)}>Cancel</Button>
              <Button type="submit">Snooze</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  )
}
