import { Refresh, Trash } from 'reicon-react'
import { EmptyState } from '../../components/ui/EmptyState'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useProjectTrash, useRestoreProject } from './api/projects'
import { useRestoreTask, useTaskTrash } from './api/tasks'

export function TaskTrashPage() {
  const { workspace } = useWorkspace()
  const trash = useTaskTrash(workspace.id)
  const restore = useRestoreTask(workspace.id)
  const projects = useProjectTrash(workspace.id)
  const restoreProject = useRestoreProject(workspace.id)
  if (trash.isPending || projects.isPending) return <EmptyState icon={Trash} title="Loading trash" description="Loading deleted projects and tasks." />
  if (trash.isError || projects.isError) return <EmptyState icon={Trash} title="Trash unavailable" description="The server could not load deleted records." />
  return (
    <div className="page"><section className="pane" style={{ flex: 1 }}><div className="pane-header"><span className="pane-title">Trash</span></div><div className="pane-body">
      {projects.data.map((project) => <div className="list-row" key={project.id}><span className="truncate" style={{ flex: 1 }}>Project: {project.name}</span><button className="button" disabled={restoreProject.isPending} onClick={() => restoreProject.mutate({ projectId: project.id, version: project.version })}><Refresh size={14} />Restore project</button></div>)}
      {trash.data.map((task) => (
        <div className="list-row" key={task.id}><span className="truncate" style={{ flex: 1 }}>{task.title || 'Untitled task'}</span><button className="button" disabled={restore.isPending} onClick={() => restore.mutate({ taskId: task.id, version: task.version })}><Refresh size={14} />Restore</button></div>
      ))}
      {trash.data.length === 0 && projects.data.length === 0 ? <EmptyState icon={Trash} title="Trash is empty" description="Deleted projects and tasks appear here until restored." /> : null}
    </div></section></div>
  )
}
