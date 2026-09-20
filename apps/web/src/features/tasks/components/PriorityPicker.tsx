import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { PriorityIcon } from './PriorityIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import type { Task } from '@/features/tasks/api/models'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`

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
        <DropdownMenuContent align={align === 'right' ? 'end' : 'start'} className={MENU}>
          {PRIORITY_ORDER.map((priority) => (
            <DropdownMenuItem
              key={priority}
              className={OPTION}
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
