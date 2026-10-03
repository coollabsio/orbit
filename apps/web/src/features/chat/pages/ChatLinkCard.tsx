import type { ComponentProps } from 'react'
import { Link } from 'react-router'
import { DocumentText } from 'reicon-react'
import { cn } from 'cn'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { usePageTree } from '@/features/docs/api/pages'
import { useTeamspaces } from '@/features/docs/api/teamspaces'
import { pageTitle, spaceKey, spaceLabel } from '@/features/docs/pageTree'
import { taskIdentifier } from '@/features/tasks/api/models'
import { useProjectStatuses, useProjects } from '@/features/tasks/api/projects'
import { useTask, useTaskRouteId } from '@/features/tasks/api/tasks'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { taskPath } from '@/lib/taskLinks'

/** The compact card under a message for a link to an Orbit task or page. One row; the whole card is the link. */
function LinkCard({ className, ...props }: ComponentProps<typeof Link>) {
  return (
    <Link
      data-slot="chat-link-card"
      className={cn(
        'flex h-9 w-full max-w-md min-w-0 items-center gap-2 rounded-lg border bg-card px-2.5 text-[13px] text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50 hover-fine:hover:bg-muted/50',
        className,
      )}
      {...props}
    />
  )
}

/** Title, status and assignees of a task (named by id or identifier). Nothing while it loads, or when the user cannot see it. */
function TaskLinkCard({ taskId: taskRef }: { taskId: string }) {
  const { workspace } = useWorkspace()
  const { taskId } = useTaskRouteId(workspace.id, taskRef)
  const task = useTask(workspace.id, taskId).data
  const projects = useProjects(workspace.id, task !== undefined).data
  const statuses = useProjectStatuses(workspace.id, task?.project_id).data
  const members = useMembers(workspace.id).data
  if (!task || task.deleted_at) return null
  const status = statuses?.find((candidate) => candidate.id === task.status_id)
  const assignees = (members ?? []).filter((member) => task.assignee_ids.includes(member.id))
  const project = projects?.find((candidate) => candidate.id === task.project_id)
  return (
    <LinkCard to={taskPath({ id: task.id, number: task.number, projectKey: project?.key })} data-kind="task">
      <TaskStatusIcon status={status} />
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{taskIdentifier(task.id, project, task.number)}</span>
      <span className="min-w-0 flex-1 truncate font-medium">{task.title}</span>
      {status ? <span className="shrink-0 text-xs text-muted-foreground max-[899px]:hidden">{status.name}</span> : null}
      {assignees.length > 0 ? <UserAvatarStack users={assignees} size={18} /> : null}
    </LinkCard>
  )
}

/** Title and teamspace of a page. Nothing while the page tree loads, or when the page is not in it. */
function PageLinkCard({ pageId }: { pageId: string }) {
  const { workspace } = useWorkspace()
  const page = usePageTree(workspace.id).data?.find((candidate) => candidate.id === pageId)
  const teamspaces = useTeamspaces(workspace.id, page !== undefined).data
  if (!page) return null
  return (
    <LinkCard to={`/docs/${page.id}`} data-kind="page">
      {page.icon ? <span aria-hidden="true">{page.icon}</span> : <DocumentText className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />}
      <span className="min-w-0 flex-1 truncate font-medium">{pageTitle(page)}</span>
      <span className="shrink-0 text-xs text-muted-foreground">{spaceLabel(spaceKey(page), teamspaces)}</span>
    </LinkCard>
  )
}

/** What `ChatHost.renderLinkCard` returns for an Orbit task or page URL. It lives beside `ChatPage` because it reads other features. */
export function ChatLinkCard({ kind, id }: { kind: 'task' | 'page'; id: string }) {
  return kind === 'task' ? <TaskLinkCard taskId={id} /> : <PageLinkCard pageId={id} />
}
