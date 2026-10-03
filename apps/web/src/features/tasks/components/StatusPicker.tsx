import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { statusPickerOptions } from '@/features/tasks/pickerLib'
import { useTaskPickerUpdate } from '@/features/tasks/useTaskPickerUpdate'
import { TaskStatusIcon } from './TaskStatusIcon'

/** What the picker needs of a task: also fits a relation's other task. */
export interface StatusPickerTask {
  id: string
  projectId: string
  statusId: string
  version: number
}

/**
 * Status glyph that opens a menu to change the status in place (list rows, board cards, timeline, sub-issue and
 * relation rows). `onRequestDuplicate` adds Duplicate, which opens the caller's canonical-task picker instead of saving.
 */
export function StatusPicker({ task, statuses, size, align = 'start', className, onRequestDuplicate }: {
  task: StatusPickerTask
  statuses: TaskStatusDef[]
  /** Glyph size (default 14). */
  size?: number
  align?: 'start' | 'end'
  /** Trigger button classes (size, margins). */
  className?: string
  onRequestDuplicate?: () => void
}) {
  const save = useTaskPickerUpdate(task, 'Status update failed.')
  const status = statuses.find((item) => item.id === task.statusId)
  const options = statusPickerOptions(statuses, task.projectId, Boolean(onRequestDuplicate))
  return (
    <div className="flex shrink-0" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" size="icon-sm" className={cn('size-[22px]', className)} aria-label={`Status: ${status?.name ?? 'None'}`} title="Change status">
              <TaskStatusIcon status={status} size={size} />
            </Button>
          }
        />
        <DropdownMenuContent align={align} className="w-auto min-w-45">
          {options.map((option) => (
            <DropdownMenuItem
              key={option.id}
              className="data-selected:bg-accent data-selected:font-medium"
              data-selected={option.id === task.statusId || undefined}
              onClick={() => {
                if (option.category === 'duplicate') onRequestDuplicate?.()
                else if (option.id !== task.statusId) save({ status_id: option.id })
              }}
            >
              <TaskStatusIcon status={option} />
              {option.name}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}
