import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { PriorityIcon } from './PriorityIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import type { Task } from '@/features/tasks/api/models'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** Priority glyph that opens a menu to change the priority in place (list rows and board cards). */
export function PriorityPicker({ task, align = 'left' }: { task: Task; align?: 'left' | 'right' }) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  return (
    <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" size="icon-sm" className="size-[22px] text-muted-foreground/70" aria-label={`Priority: ${PRIORITY_LABEL[task.priority]}`} title="Change priority">
              <PriorityIcon priority={task.priority} />
            </Button>
          }
        />
        <DropdownMenuContent align={align === 'right' ? 'end' : 'start'} className="w-auto min-w-45">
          {PRIORITY_ORDER.map((priority) => (
            <DropdownMenuItem
              key={priority}
              className="data-selected:bg-accent data-selected:font-medium"
              data-selected={priority === task.priority || undefined}
              onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, priority } })}
            >
              <PriorityIcon priority={priority} />
              {PRIORITY_LABEL[priority]}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      {updateTask.isError ? <span role="alert" className="text-xs text-destructive">Priority update failed. <Button variant="ghost" onClick={() => updateTask.variables && updateTask.mutate(updateTask.variables)}>Retry</Button></span> : null}
    </div>
  )
}
