import { Refresh as RotateCcw, Trash as Trash2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/common/EmptyState'
import { VirtualList } from '@/components/common/VirtualList'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjectTrash, useRestoreProject } from '@/features/tasks/api/projects'
import { useRestoreTask, useTaskTrash } from '@/features/tasks/api/tasks'
import { trashedSubIssuesLabel } from '@/features/tasks/subIssuesLib'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { loadFailed } from '@/lib/connection'

export function TaskTrashPage() {
  const { workspace } = useWorkspace()
  const trash = useTaskTrash(workspace.id)
  const restore = useRestoreTask(workspace.id)
  const projects = useProjectTrash(workspace.id)
  const restoreProject = useRestoreProject(workspace.id)
  const restoring = useSlowPending(restore.isPending || restoreProject.isPending)
  if (loadFailed(trash) || loadFailed(projects)) return <div><EmptyState icon={Trash2} title="Trash unavailable" description="The server could not load deleted records." /><Button variant="outline" onClick={() => { void trash.refetch(); void projects.refetch() }}>Retry</Button></div>
  if (trash.data === undefined || projects.data === undefined) return <EmptyState icon={Trash2} title="Loading trash" description="Loading deleted projects and tasks." />
  // deleted projects first, then deleted tasks
  const count = projects.data.length + trash.data.length
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><Pane><PaneHeader><PaneTitle>Trash</PaneTitle></PaneHeader>
      {restore.isError ? <p role="alert" className="text-destructive">Task restore failed. <Button variant="ghost" onClick={() => restore.variables && restore.mutate(restore.variables)}>Retry</Button></p> : null}
      {restoreProject.isError ? <p role="alert" className="text-destructive">Project restore failed. <Button variant="ghost" onClick={() => restoreProject.variables && restoreProject.mutate(restoreProject.variables)}>Retry</Button></p> : null}
      {restoring ? <p role="status">Restoring deleted record…</p> : null}
      {count === 0 ? <div className="min-h-0 flex-1 overflow-y-auto"><EmptyState icon={Trash2} title="Trash is empty" description="Deleted projects and tasks appear here until restored." /></div> : (
        <VirtualList className="min-h-0 flex-1" count={count} rowHeight={45} rowKey={(index) => (projects.data[index] ?? trash.data[index - projects.data.length]).id}>
          {(index) => {
            const project = projects.data[index]
            if (project) return <TrashRow><span className="flex-1 truncate">Project: {project.name}</span><Button variant="outline" disabled={restoreProject.isPending} onClick={() => restoreProject.mutate({ projectId: project.id, version: project.version })}><RotateCcw className="size-3.5" />Restore project</Button></TrashRow>
            const task = trash.data[index - projects.data.length]
            // read structurally: only trash entries carry it
            const label = trashedSubIssuesLabel((task as { trashed_descendant_count?: number }).trashed_descendant_count ?? 0)
            return <TrashRow><span className="min-w-0 flex-1 truncate">{task.title || 'Untitled task'}</span>{label ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{label}</span> : null}<Button variant="outline" disabled={restore.isPending} onClick={() => restore.mutate({ taskId: task.id, version: task.version })}><RotateCcw className="size-3.5" />Restore</Button></TrashRow>
          }}
        </VirtualList>
      )}
    </Pane></div>
  )
}

function TrashRow({ children }: { children: React.ReactNode }) {
  return <div data-slot="trash-row" className="flex min-h-10 w-full min-w-0 items-center gap-2.5 border-b px-3 py-1.5">{children}</div>
}
