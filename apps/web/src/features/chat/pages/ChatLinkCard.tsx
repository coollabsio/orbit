import type { ComponentProps, ReactNode } from 'react'
import { Link } from 'react-router'
import { DocumentText } from 'reicon-react'
import { cn } from 'cn'
import { usePageTree } from '@/features/docs/api/pages'
import { useTeamspaces } from '@/features/docs/api/teamspaces'
import { pageTitle, spaceKey, spaceLabel } from '@/features/docs/pageTree'
import { taskFromRecord } from '@/features/tasks/api/models'
import { useProjectStatuses, useProjects } from '@/features/tasks/api/projects'
import { useTask, useTaskRouteId } from '@/features/tasks/api/tasks'
import { AssigneePicker } from '@/features/tasks/components/AssigneePicker'
import { DueDatePicker } from '@/features/tasks/components/DueDatePicker'
import { PriorityPicker } from '@/features/tasks/components/PriorityPicker'
import { StatusPicker } from '@/features/tasks/components/StatusPicker'
import { ProjectPicker } from '@/features/tasks/components/ProjectPicker'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useSavedView } from '@/features/views/api/views'
import { viewPath } from '@/features/views/viewActions'
import { FavoriteStar } from '@/features/favorites/FavoriteStar'
import { ViewIcon } from '@/features/views/components/ViewIcon'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { relativeTime } from '@/lib/format'
import { taskPath } from '@/lib/taskLinks'

/**
 * The task a link names (by id or identifier), as the reader's own requests return it: `null` while it loads, when the
 * reader cannot see it and when it is in the trash. A card or chip then shows nothing of the task.
 */
function useLinkedTask(taskRef: string) {
  const { workspace } = useWorkspace()
  const { taskId } = useTaskRouteId(workspace.id, taskRef)
  const record = useTask(workspace.id, taskId).data
  const projects = useProjects(workspace.id, record !== undefined).data
  const statuses = useProjectStatuses(workspace.id, record?.project_id).data ?? []
  if (!record || record.deleted_at) return null
  const project = projects?.find((candidate) => candidate.id === record.project_id)
  const task = taskFromRecord(record, project)
  return { task, projects: projects ?? [], statuses, status: statuses.find((candidate) => candidate.id === task.statusId) }
}

/** The page a link names: `null` while the page tree loads and when the page is not in the reader's tree. */
function useLinkedPage(pageId: string) {
  const { workspace } = useWorkspace()
  return usePageTree(workspace.id).data?.find((candidate) => candidate.id === pageId) ?? null
}

/**
 * The card under a message for a link to an Orbit task. The reader works on the task from here: status, priority,
 * project, due date and assignees change in place with the pickers of the task list, and the title opens the task. Every
 * workspace member may edit a task, so whoever sees the card may use it; the server checks each save.
 */
function TaskLinkCard({ taskId }: { taskId: string }) {
  const { workspace } = useWorkspace()
  const linked = useLinkedTask(taskId)
  const members = useMembers(workspace.id).data ?? []
  if (!linked) return null
  const { task, projects, statuses, status } = linked
  return (
    <div data-slot="chat-link-card" data-kind="task" className="flex w-full max-w-md min-w-0 flex-col gap-0.5 rounded-lg border bg-card py-1.5 pr-2.5 pl-1.5 text-[13px] text-foreground">
      <div className="flex min-w-0 items-center gap-1.5">
        <StatusPicker task={task} statuses={statuses} />
        <Link to={taskPath(task)} className="min-w-0 flex-1 truncate rounded-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring/50 hover-fine:hover:underline">
          {task.title}
        </Link>
        <AssigneePicker task={task} users={members} />
      </div>
      <div className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
        <PriorityPicker task={task} />
        <span className="shrink-0 tabular-nums">{task.identifier}</span>
        {status ? <span className="truncate">{status.name}</span> : null}
        <ProjectPicker task={task} projects={projects} className="flex min-w-0 max-[480px]:hidden" />
        <DueDatePicker task={task} status={status} empty="icon" className="ml-auto shrink-0" />
      </div>
    </div>
  )
}

/** The card under a message for a link to a page: icon, title, space and last change. One row; the whole card is the link. */
function PageLinkCard({ pageId }: { pageId: string }) {
  const { workspace } = useWorkspace()
  const page = useLinkedPage(pageId)
  const teamspaces = useTeamspaces(workspace.id, page !== null).data
  if (!page) return null
  return (
    <Link
      to={`/docs/${page.id}`}
      data-slot="chat-link-card"
      data-kind="page"
      className="flex h-9 w-full max-w-md min-w-0 items-center gap-2 rounded-lg border bg-card px-2.5 text-[13px] text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50 hover-fine:hover:bg-muted/50"
    >
      {page.icon ? <span aria-hidden="true">{page.icon}</span> : <DocumentText className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />}
      <span className="min-w-0 flex-1 truncate font-medium">{pageTitle(page)}</span>
      <span className="shrink-0 text-xs text-muted-foreground">
        {spaceLabel(spaceKey(page), teamspaces)} · {relativeTime(page.updated_at)}
      </span>
    </Link>
  )
}

/**
 * The saved view a link names, when the whole workspace can see it: `null` while it loads, when it does not exist and
 * for a personal view (also the reader's own: the other readers of the message could not open it).
 */
function useLinkedView(viewId: string) {
  const { workspace } = useWorkspace()
  const view = useSavedView(workspace.id, viewId).data
  return view?.visibility === 'workspace' ? view : null
}

/** The card under a message for a link to a workspace view: icon, name, description or owner, and the reader's favorite star. */
function ViewLinkCard({ viewId }: { viewId: string }) {
  const { workspace } = useWorkspace()
  const view = useLinkedView(viewId)
  if (!view) return null
  return (
    <div data-slot="chat-link-card" data-kind="view" className="relative flex h-9 w-full max-w-md min-w-0 items-center gap-2 rounded-lg border bg-card pr-1.5 pl-2.5 text-[13px] text-foreground hover-fine:hover:bg-muted/50">
      <ViewIcon icon={view.icon} color={view.color} className="size-4 shrink-0" />
      {/* the link covers the card; the star sits above it */}
      <Link to={viewPath(view.id)} className="min-w-0 shrink-0 truncate font-medium outline-none after:absolute after:inset-0 after:rounded-lg focus-visible:after:ring-2 focus-visible:after:ring-ring/50 max-[480px]:shrink">
        {view.name}
      </Link>
      <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{view.description || `View by ${view.owner.display_name}`}</span>
      <FavoriteStar workspaceId={workspace.id} kind="view" targetId={view.id} on={view.is_favorite} className="relative z-10" />
    </div>
  )
}

export type LinkKind = 'task' | 'page' | 'view'

/** What `ChatHost.renderLinkCard` returns for an Orbit task, page or view URL. It lives beside `ChatPage` because it reads other features. */
export function ChatLinkCard({ kind, id }: { kind: LinkKind; id: string }) {
  if (kind === 'task') return <TaskLinkCard taskId={id} />
  return kind === 'page' ? <PageLinkCard pageId={id} /> : <ViewLinkCard viewId={id} />
}

/** A link in the text of a message that names its target in place of the URL, in the look of a mention. */
function LinkChip({ className, ...props }: ComponentProps<typeof Link>) {
  return (
    <Link
      data-slot="chat-link-chip"
      className={cn('rounded-sm bg-primary/10 px-1 font-medium text-primary no-underline hover:underline dark:bg-primary/20 [&>svg]:mr-1 [&>svg]:inline [&>svg]:align-[-2px]', className)}
      {...props}
    />
  )
}

interface ChipProps {
  id: string
  /** The link is a whole line of the message, so its card stands in for it. */
  hidden: boolean
  /** The plain URL link: shown until the target is known, and when the reader cannot see it. */
  children: ReactNode
}

function TaskLinkChip({ id, hidden, children }: ChipProps) {
  const linked = useLinkedTask(id)
  if (!linked) return children
  if (hidden) return null
  return (
    <LinkChip to={taskPath(linked.task)} data-kind="task">
      <TaskStatusIcon status={linked.status} size={12} />
      <span className="opacity-70">{linked.task.identifier}</span> {linked.task.title}
    </LinkChip>
  )
}

function PageLinkChip({ id, hidden, children }: ChipProps) {
  const page = useLinkedPage(id)
  if (!page) return children
  if (hidden) return null
  return (
    <LinkChip to={`/docs/${page.id}`} data-kind="page">
      {page.icon ? <span aria-hidden="true">{page.icon} </span> : <DocumentText className="size-3.5" aria-hidden="true" />}
      {pageTitle(page)}
    </LinkChip>
  )
}

function ViewLinkChip({ id, hidden, children }: ChipProps) {
  const view = useLinkedView(id)
  if (!view) return children
  if (hidden) return null
  return (
    <LinkChip to={viewPath(view.id)} data-kind="view">
      <ViewIcon icon={view.icon} color={view.color} className="size-3.5" />
      {view.name}
    </LinkChip>
  )
}

/** What `ChatHost.renderLink` returns for an Orbit task, page or view URL in the text of a message. */
export function ChatLinkChip({ kind, ...props }: ChipProps & { kind: LinkKind }) {
  if (kind === 'task') return <TaskLinkChip {...props} />
  return kind === 'page' ? <PageLinkChip {...props} /> : <ViewLinkChip {...props} />
}
