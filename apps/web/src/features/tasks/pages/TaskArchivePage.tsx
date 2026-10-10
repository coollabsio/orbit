import { Link, useSearchParams } from 'react-router'
import { ArchiveBox, ArchiveUp } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/common/EmptyState'
import { VirtualList } from '@/components/common/VirtualList'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjects } from '@/features/tasks/api/projects'
import { useTaskArchive } from '@/features/tasks/api/tasks'
import { useArchiveActions } from '@/features/tasks/useArchiveActions'
import { loadFailed } from '@/lib/connection'
import { formatTaskIdentifier, taskPath } from '@/lib/taskLinks'

/** The archive: closed tasks that left the lists. `?project=` shows one project. A restore brings back the task's tree. */
export function TaskArchivePage() {
  const { workspace } = useWorkspace()
  const [searchParams] = useSearchParams()
  const projectId = searchParams.get('project')
  const archive = useTaskArchive(workspace.id, projectId)
  const projects = useProjects(workspace.id).data ?? []
  const { restore, pending } = useArchiveActions(workspace.id)
  const project = projects.find((item) => item.id === projectId)
  const title = project ? `Archive: ${project.name}` : 'Archive'
  if (loadFailed(archive)) return <div><EmptyState icon={ArchiveBox} title="Archive unavailable" description="The server could not load the archived tasks." /><Button variant="outline" onClick={() => void archive.refetch()}>Retry</Button></div>
  if (archive.data === undefined) return <EmptyState icon={ArchiveBox} title="Loading archive" description="Loading archived tasks." />
  const tasks = archive.data
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><Pane><PaneHeader><PaneTitle>{title}</PaneTitle></PaneHeader>
      {tasks.length === 0 ? <div className="min-h-0 flex-1 overflow-y-auto"><EmptyState icon={ArchiveBox} title="Archive is empty" description="Closed tasks that you archive, or that a project archives after its period, appear here." /></div> : (
        <VirtualList className="min-h-0 flex-1" count={tasks.length} rowHeight={45} rowKey={(index) => tasks[index].id}>
          {(index) => {
            const task = tasks[index]
            const key = projects.find((item) => item.id === task.project_id)?.key
            const identifier = key && task.number ? formatTaskIdentifier(key, task.number) : null
            return (
              <div data-slot="archive-row" className="flex min-h-10 w-full min-w-0 items-center gap-2.5 border-b px-3 py-1.5">
                {identifier ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{identifier}</span> : null}
                <Link className="min-w-0 flex-1 truncate hover:underline" to={taskPath({ id: task.id, number: task.number, projectKey: key })}>{task.title || 'Untitled task'}</Link>
                <Button variant="outline" disabled={pending} aria-label={`Restore ${task.title}`} onClick={() => void restore([task])}><ArchiveUp className="size-3.5" />Restore</Button>
              </div>
            )
          }}
        </VirtualList>
      )}
    </Pane></div>
  )
}
