import { useState } from 'react'
import { Link } from 'react-router'
import { Refresh as RotateCcw, Search4 as Search, Trash as Trash2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/common/EmptyState'
import { VirtualList } from '@/components/common/VirtualList'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { confirmAction } from '@/components/common/confirmAction'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCan } from '@/features/workspaces/permissions'
import { usePurgeProject, useProjects, useProjectTrash, useRestoreProject } from '@/features/tasks/api/projects'
import { useEmptyTaskTrash, usePurgeTask, useRestoreTask, useTaskTrash } from '@/features/tasks/api/tasks'
import { TaskSearchBox } from '@/features/tasks/components/TaskSearchBox'
import type { ProjectRecord, TaskRecord } from '@orbit/api/generated/types.gen'
import { filterTrash, trashIdentifier } from '@orbit/api/trash'
import { trashedSubIssuesLabel } from '@/features/tasks/subIssuesLib'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { loadFailed } from '@/lib/connection'

export function TaskTrashPage() {
  const { workspace } = useWorkspace()
  const trash = useTaskTrash(workspace.id)
  const restore = useRestoreTask(workspace.id)
  const projects = useProjectTrash(workspace.id)
  const restoreProject = useRestoreProject(workspace.id)
  const purgeProject = usePurgeProject(workspace.id)
  const purgeTask = usePurgeTask(workspace.id)
  const emptyTrash = useEmptyTaskTrash(workspace.id)
  const liveProjects = useProjects(workspace.id)
  const [search, setSearch] = useState('')
  // the same role check as the server's can_purge_task
  const canPurge = useCan('pages.purge')
  const restoring = useSlowPending(restore.isPending || restoreProject.isPending)
  const deleting = useSlowPending(purgeProject.isPending || purgeTask.isPending || emptyTrash.isPending)
  const busy = restore.isPending || restoreProject.isPending || purgeProject.isPending || purgeTask.isPending || emptyTrash.isPending
  if (loadFailed(trash) || loadFailed(projects)) return <div><EmptyState icon={Trash2} title="Trash unavailable" description="The server could not load deleted records." /><Button variant="outline" onClick={() => { void trash.refetch(); void projects.refetch() }}>Retry</Button></div>
  if (trash.data === undefined || projects.data === undefined) return <EmptyState icon={Trash2} title="Loading trash" description="Loading deleted projects and tasks." />
  const total = projects.data.length + trash.data.length
  const keyOf = (projectId: string) => liveProjects.data?.find((project) => project.id === projectId)?.key
  const shown = filterTrash(projects.data, trash.data, search, keyOf)
  // deleted projects first, then deleted tasks
  const count = shown.projects.length + shown.tasks.length
  const requestPurge = async (project: ProjectRecord) => {
    const confirmed = await confirmAction({
      title: `Delete “${project.name}” forever?`,
      description: 'The project and all its tasks are deleted permanently. They can’t be restored.',
      confirmLabel: 'Delete forever',
      danger: true,
    })
    if (confirmed) purgeProject.mutate(project.id)
  }
  const requestPurgeTask = async (task: TaskRecord) => {
    const below = (task as { trashed_descendant_count?: number }).trashed_descendant_count ?? 0
    const confirmed = await confirmAction({
      title: `Delete “${task.title || 'Untitled task'}” forever?`,
      description: below > 0
        ? `The task and its ${below === 1 ? 'sub-issue' : `${below} sub-issues`} are deleted permanently. They can’t be restored.`
        : 'The task is deleted permanently. It can’t be restored.',
      confirmLabel: 'Delete forever',
      danger: true,
    })
    if (confirmed) purgeTask.mutate(task.id)
  }
  const requestEmpty = async () => {
    const parts = [
      projects.data.length > 0 ? `${projects.data.length} ${projects.data.length === 1 ? 'project' : 'projects'} with all their tasks` : null,
      trash.data.length > 0 ? `${trash.data.length} ${trash.data.length === 1 ? 'task' : 'tasks'}` : null,
    ].filter(Boolean)
    const confirmed = await confirmAction({
      title: 'Empty trash?',
      description: `${parts.join(' and ')} will be deleted forever. They can’t be restored.`,
      confirmLabel: 'Empty trash',
      danger: true,
    })
    if (confirmed) emptyTrash.mutate()
  }
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><Pane><PaneHeader className="max-[899px]:flex-wrap"><PaneTitle className="min-w-0 flex-1">Trash</PaneTitle>{total > 0 ? <TaskSearchBox value={search} onChange={setSearch} label="Search trash" /> : null}{canPurge && total > 0 ? <Button type="button" variant="ghost" size="sm" className="text-destructive hover:text-destructive" disabled={busy} onClick={() => void requestEmpty()}>Empty trash</Button> : null}</PaneHeader>
      {restore.isError ? <p role="alert" className="text-destructive">Task restore failed. <Button variant="ghost" onClick={() => restore.variables && restore.mutate(restore.variables)}>Retry</Button></p> : null}
      {restoreProject.isError ? <p role="alert" className="text-destructive">Project restore failed. <Button variant="ghost" onClick={() => restoreProject.variables && restoreProject.mutate(restoreProject.variables)}>Retry</Button></p> : null}
      {purgeProject.isError ? <p role="alert" className="text-destructive">Deleting the project failed. <Button variant="ghost" onClick={() => purgeProject.variables && purgeProject.mutate(purgeProject.variables)}>Retry</Button></p> : null}
      {purgeTask.isError ? <p role="alert" className="text-destructive">Deleting the task failed. <Button variant="ghost" onClick={() => purgeTask.variables && purgeTask.mutate(purgeTask.variables)}>Retry</Button></p> : null}
      {emptyTrash.isError ? <p role="alert" className="text-destructive">Emptying the trash failed. <Button variant="ghost" onClick={() => emptyTrash.mutate()}>Retry</Button></p> : null}
      {restoring ? <p role="status">Restoring deleted record…</p> : null}
      {deleting ? <p role="status">Deleting forever…</p> : null}
      {total === 0 ? <div className="min-h-0 flex-1 overflow-y-auto"><EmptyState icon={Trash2} title="Trash is empty" description="Deleted projects and tasks appear here until restored." /></div> : count === 0 ? <div className="min-h-0 flex-1 overflow-y-auto"><EmptyState icon={Search} title="No results" description="No deleted project or task matches your search." /></div> : (
        <VirtualList className="min-h-0 flex-1" count={count} rowHeight={45} rowKey={(index) => (shown.projects[index] ?? shown.tasks[index - shown.projects.length]).id}>
          {(index) => {
            const project = shown.projects[index]
            if (project) return <TrashRow><Link to={`/tasks-trash/project/${project.id}`} className="min-w-0 flex-1 truncate hover:underline">Project: {project.name}</Link><Button variant="outline" disabled={busy} onClick={() => restoreProject.mutate({ projectId: project.id, version: project.version })}><RotateCcw className="size-3.5" />Restore project</Button>{canPurge ? <Button variant="outline" className="text-destructive hover:text-destructive" disabled={busy} onClick={() => void requestPurge(project)}><Trash2 className="size-3.5" />Delete forever</Button> : null}</TrashRow>
            const task = shown.tasks[index - shown.projects.length]
            // read structurally: only trash entries carry it
            const label = trashedSubIssuesLabel((task as { trashed_descendant_count?: number }).trashed_descendant_count ?? 0)
            const identifier = trashIdentifier(task, keyOf)
            return <TrashRow><Link to={`/tasks-trash/task/${task.id}`} className="flex min-w-0 flex-1 items-baseline gap-2 hover:underline">{identifier ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{identifier}</span> : null}<span className="truncate">{task.title || 'Untitled task'}</span></Link>{label ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{label}</span> : null}<Button variant="outline" disabled={busy} onClick={() => restore.mutate({ taskId: task.id, version: task.version })}><RotateCcw className="size-3.5" />Restore</Button>{canPurge ? <Button variant="outline" className="text-destructive hover:text-destructive" disabled={busy} onClick={() => void requestPurgeTask(task)}><Trash2 className="size-3.5" />Delete forever</Button> : null}</TrashRow>
          }}
        </VirtualList>
      )}
    </Pane></div>
  )
}

function TrashRow({ children }: { children: React.ReactNode }) {
  return <div data-slot="trash-row" className="flex min-h-10 w-full min-w-0 items-center gap-2.5 border-b px-3 py-1.5">{children}</div>
}
