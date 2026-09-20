import { Link } from 'react-router'
import { Hashtag as Hash } from 'reicon-react'
import { useAppState } from '@/mock/store'
import { relativeTime } from '@/lib/format'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'
import { buttonVariants } from '@/components/ui/button'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { PriorityIcon } from '@/features/tasks/components/PriorityIcon'
import { PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import { useCurrentUser } from '@/features/auth/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { taskFromRecord } from '@/features/tasks/api/models'
import { useTasks } from '@/features/tasks/api/tasks'

const LIST_ROW = 'flex min-h-10 w-full min-w-0 cursor-pointer items-center gap-2.5 border-b border-border px-3 py-1.5 text-left transition-colors last:border-b-0 hover:bg-foreground/[0.02]'
const CARD_EMPTY = 'p-4 text-[13px] text-muted-foreground/70'
const META = 'shrink-0 text-xs text-muted-foreground/70'

function greetingFor(hour: number): string {
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

function HomeCard({
  title,
  viewAllTo,
  children,
}: {
  title: string
  viewAllTo: string
  children: React.ReactNode
}) {
  return (
    <section className="flex w-full min-w-0 flex-col rounded-lg bg-card shadow-[0_0_0_1px_var(--border)] transition-shadow duration-200 hover:shadow-[0_0_0_1px_var(--border),0_8px_24px_rgba(0,0,0,0.08)]">
      <header className="flex min-h-12 items-center gap-2 py-2 pr-2 pl-4 text-sm font-medium text-muted-foreground">
        {title}
        <span className="flex-1" />
        <Link to={viewAllTo} className={buttonVariants({ variant: 'ghost' })}>
          View all
        </Link>
      </header>
      <div className="relative min-w-0 overflow-hidden rounded-lg bg-background shadow-[0_0_0_1px_var(--muted)]">{children}</div>
    </section>
  )
}

export function HomePage() {
  const state = useAppState()
  const { workspace } = useWorkspace()
  const me = useCurrentUser()
  const projects = useProjects(workspace.id)
  const statuses = useAllStatuses(workspace.id, projects.data ?? [])
  const tasks = useTasks(workspace.id, { assignee_id: me.data?.id, limit: 100 })
  const firstName = me.data?.display_name.split(' ')[0] ?? 'there'

  const myOpenTasks = (tasks.data?.pages.flatMap((page) => page.items.map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id)))) ?? [])
    .filter(
      (t) =>
        t.assigneeIds.includes(me.data?.id ?? '') &&
        ['unstarted', 'started'].includes(statuses.data.find((s) => s.id === t.statusId)?.category ?? ''),
    )
    .sort((a, b) => {
      const p = PRIORITY_ORDER.indexOf(a.priority) - PRIORITY_ORDER.indexOf(b.priority)
      if (p !== 0) return p
      return b.updatedAt.localeCompare(a.updatedAt)
    })

  const unreadMail = state.mailThreads.filter((t) => t.unread && t.folderId === 'f_inbox').length
  const unreadChat = state.channels.reduce((sum, c) => sum + c.unreadCount, 0)
  const unreadNotifications = state.notifications.filter((n) => !n.readAt).length

  const sortedNotifications = [...state.notifications].sort((a, b) =>
    b.createdAt.localeCompare(a.createdAt),
  )
  const unreadFirst = sortedNotifications.filter((n) => !n.readAt).slice(0, 5)
  const inboxItems = unreadFirst.length > 0 ? unreadFirst : sortedNotifications.slice(0, 5)

  const recentMail = state.mailThreads
    .filter((t) => t.folderId === 'f_inbox')
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    .slice(0, 4)

  const channels = [...state.channels].sort((a, b) => b.unreadCount - a.unreadCount)

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto flex max-w-[960px] flex-col gap-6 px-6 pt-12 pb-8 max-[599px]:px-4 max-[599px]:pt-8 max-[599px]:pb-6">
            <div>
              <h1 className="mb-1.5">
                {greetingFor(new Date().getHours())}, {firstName}
              </h1>
              <p className="text-[13px] text-muted-foreground [&_a]:text-muted-foreground [&_a]:no-underline [&_a:hover]:text-foreground [&_a:hover]:underline">
                <Link to="/tasks">
                  {myOpenTasks.length} open {myOpenTasks.length === 1 ? 'task' : 'tasks'} assigned to you
                </Link>
                {' · '}
                <Link to="/mail">
                  {unreadMail} unread {unreadMail === 1 ? 'email' : 'emails'}
                </Link>
                {' · '}
                <Link to="/chat">
                  {unreadChat} unread {unreadChat === 1 ? 'message' : 'messages'}
                </Link>
                {' · '}
                <Link to="/inbox">
                  {unreadNotifications} {unreadNotifications === 1 ? 'notification' : 'notifications'}
                </Link>
              </p>
            </div>
            <div className="grid grid-cols-1 items-start gap-4 min-[900px]:grid-cols-2">
              <HomeCard title="My tasks" viewAllTo="/tasks">
                {myOpenTasks.length === 0 ? (
                  <div className={CARD_EMPTY}>No open tasks — enjoy the calm.</div>
                ) : (
                  myOpenTasks.map((task) => (
                    <Link key={task.id} to={`/tasks/${task.id}`} className={LIST_ROW}>
                      <TaskStatusIcon status={statuses.data.find((s) => s.id === task.statusId)} />
                      <span className={META}>
                        {task.identifier}
                      </span>
                      <span className="flex-1 truncate text-[13px]">
                        {task.title}
                      </span>
                      <PriorityIcon priority={task.priority} />
                      <span className={META}>
                        {relativeTime(task.updatedAt)}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>

              <HomeCard title="Inbox" viewAllTo="/inbox">
                {inboxItems.length === 0 ? (
                  <div className={CARD_EMPTY}>Nothing here yet.</div>
                ) : (
                  inboxItems.map((n) => (
                    <Link key={n.id} to="/inbox" className={LIST_ROW}>
                      <span className={cn('size-2 shrink-0', !n.readAt && 'rounded-full bg-primary')} />
                      <span className="flex-1 truncate text-[13px]">
                        {n.title}
                      </span>
                      <span className={META}>
                        {relativeTime(n.createdAt)}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>

              <HomeCard title="Recent mail" viewAllTo="/mail">
                {recentMail.length === 0 ? (
                  <div className={CARD_EMPTY}>No mail yet.</div>
                ) : (
                  recentMail.map((thread) => (
                    <Link key={thread.id} to={`/mail/${thread.id}`} className={LIST_ROW}>
                      <span className={cn('max-w-[140px] shrink-0 truncate text-[13px]', thread.unread ? 'font-semibold' : 'font-normal')}>
                        {thread.messages[0]?.from.name ?? 'Unknown'}
                      </span>
                      <span className="flex-1 truncate text-[13px] text-muted-foreground">
                        {thread.subject}
                      </span>
                      <span className={META}>
                        {relativeTime(thread.updatedAt)}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>

              <HomeCard title="Active channels" viewAllTo="/chat">
                {channels.length === 0 ? (
                  <div className={CARD_EMPTY}>No channels yet.</div>
                ) : (
                  channels.map((channel) => (
                    <Link key={channel.id} to={`/chat/${channel.id}`} className={LIST_ROW}>
                      <Hash className="size-4 shrink-0 text-muted-foreground/70" />
                      <span className={cn('shrink-0 text-[13px]', channel.unreadCount > 0 ? 'font-semibold' : 'font-normal')}>
                        {channel.name}
                      </span>
                      {channel.unreadCount > 0 ? (
                        <Badge className="h-4 min-w-4 rounded-full border-0 px-1 py-0 text-[10px] font-semibold">{channel.unreadCount}</Badge>
                      ) : null}
                      <span className="flex-1 truncate text-xs text-muted-foreground/70">
                        {channel.description}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
