import type { ComponentProps } from 'react'
import { Link } from 'react-router'
import { DirectInbox as Inbox, DocumentText as FileText } from 'reicon-react'
import { relativeTime } from '@/lib/format'
import { cn } from 'cn'
import { buttonVariants } from '@/components/ui/button'
import { Pane } from '@/components/common/Pane'
import { Emoji } from '@/components/common/Emoji'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useCurrentUser } from '@/features/auth/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useMembers } from '@/features/workspaces/api'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { taskFromRecord, type Task } from '@/features/tasks/api/models'
import { useTasks } from '@/features/tasks/api/tasks'
import { notificationCopy, notificationTarget, useNotifications } from '@/features/inbox/api'
import { usePageTree } from '@/features/docs/api/pages'
import { taskPath } from '@/lib/taskLinks'
import { docsHidden } from '@/app/shell/productNavigation'

/** Most rows a card shows: Home is a short brief, the full lists live in Tasks, Inbox and Docs. */
const CARD_LIMIT = 7

/** One linked row inside a home card. */
function CardRow({ className, ...props }: ComponentProps<typeof Link>) {
  return <Link data-slot="card-row" className={cn('flex min-h-10 w-full min-w-0 cursor-pointer items-center gap-2.5 border-b px-3 py-1.5 text-left transition-colors last:border-b-0 hover:bg-foreground/[0.02]', className)} {...props} />
}

function CardEmpty({ children }: { children: React.ReactNode }) {
  return <div data-slot="card-empty" className="p-4 text-[13px] text-muted-foreground/70">{children}</div>
}

/** Quiet trailing detail of a row (identifier, time). */
function RowMeta({ className, children }: { className?: string; children: React.ReactNode }) {
  return <span data-slot="row-meta" className={cn('shrink-0 text-xs text-muted-foreground/70', className)}>{children}</span>
}

function greetingFor(hour: number): string {
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

/** `Overdue`, `Today`, `Tomorrow` or the weekday and date. */
function dueLabel(dueAt: string, now: Date): string {
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  const days = Math.round((day(new Date(dueAt)) - day(now)) / 86_400_000)
  if (days < 0) return 'Overdue'
  if (days === 0) return 'Today'
  if (days === 1) return 'Tomorrow'
  return new Date(dueAt).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })
}

function TaskRow({ task, status, now }: { task: Task; status: ComponentProps<typeof TaskStatusIcon>['status']; now: Date }) {
  return (
    <CardRow to={taskPath(task)}>
      <TaskStatusIcon status={status} />
      <RowMeta>{task.identifier}</RowMeta>
      <span className="flex-1 truncate text-[13px]">{task.title}</span>
      {task.dueAt ? <RowMeta className={cn(dueLabel(task.dueAt, now) === 'Overdue' && 'text-destructive')}>{dueLabel(task.dueAt, now)}</RowMeta> : null}
    </CardRow>
  )
}

function HomeCard({ title, viewAllTo, children }: { title: string; viewAllTo: string; children: React.ReactNode }) {
  return (
    <section className="flex w-full min-w-0 flex-col rounded-lg bg-card shadow-[0_0_0_1px_var(--border)]">
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
  const { workspace } = useWorkspace()
  const me = useCurrentUser()
  const projects = useProjects(workspace.id)
  const statuses = useAllStatuses(workspace.id, projects.data ?? [])
  const members = useMembers(workspace.id)
  const pageTree = usePageTree(workspace.id, !docsHidden)
  const notifications = useNotifications(workspace.id, true)
  const myId = me.data?.id
  // each card shows the first rows of its Tasks page; the overdue page has no assignee, so the card adds it
  const overdue = useTasks(workspace.id, { view: 'overdue', assignee_id: myId, sort: 'due_date', order: 'asc', limit: CARD_LIMIT })
  const thisWeek = useTasks(workspace.id, { view: 'my_week', sort: 'due_date', order: 'asc', limit: CARD_LIMIT })
  const firstName = me.data?.display_name.split(' ')[0] ?? 'there'
  const now = new Date()

  const toTasks = (query: typeof overdue): Task[] => query.data?.pages[0]?.items.map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id))) ?? []
  // the overdue query runs once before the user loads, without the assignee
  const overdueTasks = toTasks(overdue).filter((task) => task.assigneeIds.includes(myId ?? ''))
  const weekTasks = toTasks(thisWeek)
  // a full card with a next page means there are more tasks than the card shows
  const weekCount = `${weekTasks.length}${thisWeek.data?.pages[0]?.next_cursor ? '+' : ''}`
  const unread = notifications.data ?? []
  const recentDocs = [...(pageTree.data ?? [])].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, CARD_LIMIT)

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto flex max-w-[960px] flex-col gap-6 px-6 pt-12 pb-8 max-[599px]:px-4 max-[599px]:pt-8 max-[599px]:pb-6">
            <div>
              <h1 className="mb-1.5">
                {greetingFor(now.getHours())}, {firstName}
              </h1>
              <p className="text-[13px] text-muted-foreground [&_a]:text-muted-foreground [&_a]:no-underline [&_a:hover]:text-foreground [&_a:hover]:underline">
                <Link to="/tasks?view=my_week">
                  {weekCount} {weekCount === '1' ? 'task' : 'tasks'} due this week
                </Link>
                {' · '}
                <Link to="/inbox">
                  {unread.length} unread {unread.length === 1 ? 'notification' : 'notifications'}
                </Link>
              </p>
            </div>
            <div className="grid grid-cols-1 items-start gap-4 min-[900px]:grid-cols-2">
              {overdueTasks.length === 0 ? null : (
                <HomeCard title="Overdue" viewAllTo="/tasks?view=overdue">
                  {overdueTasks.map((task) => (
                    <TaskRow key={task.id} task={task} status={statuses.data.find((s) => s.id === task.statusId)} now={now} />
                  ))}
                </HomeCard>
              )}

              <HomeCard title="Due this week" viewAllTo="/tasks?view=my_week">
                {weekTasks.length === 0 ? (
                  <CardEmpty>Nothing due this week.</CardEmpty>
                ) : (
                  weekTasks.map((task) => <TaskRow key={task.id} task={task} status={statuses.data.find((s) => s.id === task.statusId)} now={now} />)
                )}
              </HomeCard>

              <HomeCard title="Unread in Inbox" viewAllTo="/inbox">
                {unread.length === 0 ? (
                  <CardEmpty>You are all caught up.</CardEmpty>
                ) : (
                  unread.slice(0, CARD_LIMIT).map((notification) => {
                    const page = pageTree.data?.find((item) => item.id === notification.page_id)
                    const actor = members.data?.find((member) => member.id === notification.actor_user_id)
                    return (
                      <CardRow key={notification.id} to={notificationTarget(notification) ?? '/inbox'}>
                        <Inbox className="size-4 shrink-0 text-muted-foreground/70" />
                        <span className="flex-1 truncate text-[13px]">{notificationCopy(notification, page?.title, actor?.name).title}</span>
                        <RowMeta>{relativeTime(notification.created_at)}</RowMeta>
                      </CardRow>
                    )
                  })
                )}
              </HomeCard>

              {docsHidden ? null : <HomeCard title="Recently updated docs" viewAllTo="/docs">
                {recentDocs.length === 0 ? (
                  <CardEmpty>No docs yet.</CardEmpty>
                ) : (
                  recentDocs.map((page) => (
                    <CardRow key={page.id} to={`/docs/${page.id}`}>
                      {page.icon ? <Emoji value={page.icon} /> : <FileText className="size-4 shrink-0 text-muted-foreground/70" />}
                      <span className="flex-1 truncate text-[13px]">{page.title.trim() || 'Untitled'}</span>
                      <RowMeta>{relativeTime(page.updated_at)}</RowMeta>
                    </CardRow>
                  ))
                )}
              </HomeCard>}
            </div>
          </div>
        </div>
      </Pane>
    </div>
  )
}
