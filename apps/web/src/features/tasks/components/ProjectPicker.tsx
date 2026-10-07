import { ColorDot } from '@/components/common/ColorDot'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import type { Project, Task } from '@/features/tasks/api/models'
import { useMoveToProject } from '@/features/tasks/useMoveToProject'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { ProjectChip } from './TaskPropertyChips'

/**
 * Project chip that opens a menu to move the task to another project in place. The task gets the new project's next
 * number; a task the server keeps in its project (synced with GitHub) stays, with a toast that says why.
 */
export function ProjectPicker({ task, projects, className }: { task: Pick<Task, 'id' | 'version' | 'projectId'>; projects: Project[]; className?: string }) {
  const { workspace } = useWorkspace()
  const moveToProject = useMoveToProject(workspace.id)
  const project = projects.find((item) => item.id === task.projectId)
  if (projects.length < 2) return <ProjectChip project={project} className={className} />
  return (
    <div className={className} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DropdownMenu modal={false}>
        <Tip label="Move to project">
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="link" className="h-auto min-w-0 p-0 text-[length:inherit] font-normal" aria-label={`Project: ${project?.name ?? 'none'}`}>
                <ProjectChip project={project} className="hover:text-foreground" />
              </Button>
            }
          />
        </Tip>
        <DropdownMenuContent className="w-auto min-w-52">
          <DropdownMenuGroup>
            <DropdownMenuLabel>Move to project</DropdownMenuLabel>
            {projects.map((item) => (
              <DropdownMenuItem
                key={item.id}
                className="data-selected:bg-accent data-selected:font-medium"
                data-selected={item.id === task.projectId || undefined}
                onClick={() => moveToProject([task], item)}
              >
                <ColorDot color={item.color} className="size-2" />
                <span className="flex-1 truncate">{item.name}</span>
                <span className="text-xs text-muted-foreground tabular-nums">{item.key}</span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}
