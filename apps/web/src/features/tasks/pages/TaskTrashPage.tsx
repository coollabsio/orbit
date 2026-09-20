import { RotateCcw, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/common/EmptyState'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjectTrash, useRestoreProject } from '@/features/tasks/api/projects'
import { useRestoreTask, useTaskTrash } from '@/features/tasks/api/tasks'

const ROW = 'flex min-h-10 w-full min-w-0 items-center gap-2.5 border-b border-border px-3 py-1.5'

export function TaskTrashPage() {
  const { workspace } = useWorkspace()
  const trash = useTaskTrash(workspace.id)
  const restore = useRestoreTask(workspace.id)
  const projects = useProjectTrash(workspace.id)
  const restoreProject = useRestoreProject(workspace.id)
  if (trash.isPending || projects.isPending) return <EmptyState icon={Trash2} title="Loading trash" description="Loading deleted projects and tasks." />
  if (trash.isError || projects.isError) return <div><EmptyState icon={Trash2} title="Trash unavailable" description="The server could not load deleted records." /><Button variant="outline" onClick={() => { void trash.refetch(); void projects.refetch() }}>Retry</Button></div>
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background"><div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2 max-[899px]:border-b-0"><span className="truncate text-[13px] font-semibold text-foreground">Trash</span></div><div className="min-h-0 flex-1 overflow-y-auto">
      {projects.data.map((project) => <div className={ROW} key={project.id}><span className="flex-1 truncate">Project: {project.name}</span><Button variant="outline" disabled={restoreProject.isPending} onClick={() => restoreProject.mutate({ projectId: project.id, version: project.version })}><RotateCcw className="size-3.5" />Restore project</Button></div>)}
      {trash.data.map((task) => (
        <div className={ROW} key={task.id}><span className="flex-1 truncate">{task.title || 'Untitled task'}</span><Button variant="outline" disabled={restore.isPending} onClick={() => restore.mutate({ taskId: task.id, version: task.version })}><RotateCcw className="size-3.5" />Restore</Button></div>
      ))}
      {trash.data.length === 0 && projects.data.length === 0 ? <EmptyState icon={Trash2} title="Trash is empty" description="Deleted projects and tasks appear here until restored." /> : null}
      {restore.isError ? <p role="alert" className="text-destructive">Task restore failed. <Button variant="ghost" onClick={() => restore.variables && restore.mutate(restore.variables)}>Retry</Button></p> : null}
      {restoreProject.isError ? <p role="alert" className="text-destructive">Project restore failed. <Button variant="ghost" onClick={() => restoreProject.variables && restoreProject.mutate(restoreProject.variables)}>Retry</Button></p> : null}
      {restore.isPending || restoreProject.isPending ? <p role="status">Restoring deleted record…</p> : null}
    </div></section></div>
  )
}
