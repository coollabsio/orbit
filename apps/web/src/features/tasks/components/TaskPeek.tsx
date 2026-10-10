import { taskFromRecord, type Project, type TaskViewState } from '@/features/tasks/api/models'
import { useCommentAttachments, useSubIssues, useTask, useTaskActivity, useTaskAttachments, useTaskComments, useTaskGithubLinks, useTaskRelations } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { TaskDetail } from './TaskDetail'

/**
 * A task beside the list it was peeked from (Space on a row): the task view in its one-column form. The caller
 * prefetches the task (`prefetchTaskDetail`), so the panel shows complete; while a query is still out, it is blank.
 * Desktop only: on a narrow window there is no room beside the list.
 */
export function TaskPeek({ taskId, projects, state, onClose, onOpenTask, onOpenProject }: {
  taskId: string
  projects: Project[]
  state: TaskViewState
  onClose: () => void
  onOpenTask: (taskId: string) => void
  onOpenProject: (projectId: string) => void
}) {
  const { workspace } = useWorkspace()
  const detail = useTask(workspace.id, taskId)
  const comments = useTaskComments(workspace.id, taskId)
  const activity = useTaskActivity(workspace.id, taskId)
  const attachments = useTaskAttachments(workspace.id, taskId)
  const commentAttachments = useCommentAttachments(workspace.id, taskId, comments.data ?? [])
  // TaskDetail reads these from the same cache; waiting here keeps its first paint stable
  const githubLinks = useTaskGithubLinks(workspace.id, taskId)
  const relations = useTaskRelations(workspace.id, taskId)
  const subIssues = useSubIssues(workspace.id, taskId)
  const pending = detail.isPending || comments.isPending || activity.isPending || attachments.isPending
    || commentAttachments.isPending || githubLinks.isPending || relations.isPending || subIssues.isPending
  const record = detail.data
  const project = projects.find((item) => item.id === record?.project_id)
  // tasks the activity feed may name
  const knownNumbers = new Map<string, number>([
    ...state.tasks.flatMap((task) => (task.number ? [[task.id, task.number] as const] : [])),
    ...(relations.data ?? []).map((relation) => [relation.task.id, relation.task.number] as const),
    ...(subIssues.data?.items ?? []).map((item) => [item.id, item.number] as const),
  ])
  const task = record && !pending
    ? taskFromRecord(record, project, comments.data, [...(attachments.data ?? []), ...commentAttachments.data], activity.data, projects, knownNumbers)
    : undefined

  return (
    <aside data-slot="task-peek" aria-label="Task peek" className="flex w-[480px] shrink-0 border-l max-[899px]:hidden">
      {task
        ? <TaskDetail key={taskId} peek task={task} project={project} state={state} onBack={onClose} onOpenTask={onOpenTask} onOpenProject={onOpenProject} />
        // a task that is gone (deleted, or a stale link) leaves an empty panel; Esc closes it
        : null}
    </aside>
  )
}
