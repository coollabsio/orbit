import { useState } from 'react'
import { Calendar } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DatePicker } from '@/components/common/DatePicker'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { DueDateChip } from './TaskPropertyChips'

/**
 * Due-date chip that opens the date picker in place (list rows and board cards). Without a date it shows a
 * quiet "No date" placeholder. Font size comes from `className`.
 */
export function DueDatePicker({ task, status, className }: { task: Task; status: TaskStatusDef | undefined; className?: string }) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const [open, setOpen] = useState(false)
  const setDueDate = (dueStartAt: string | null, dueAt: string | null) => {
    updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, due_start_at: dueStartAt, due_at: dueAt } })
    setOpen(false)
  }
  return (
    <div className={cn('flex', className)} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <Button
              type="button"
              variant="link"
              className="h-auto p-0 text-[length:inherit] font-normal"
              aria-label={task.dueAt ? 'Due date' : 'Set due date'}
              title={task.dueAt ? undefined : 'Set due date'}
            >
              {task.dueAt
                ? <DueDateChip task={task} status={status} className="hover:text-foreground" />
                : (
                  <span className="inline-flex items-center gap-1 whitespace-nowrap text-muted-foreground/50 transition-colors group-hover/row:text-muted-foreground hover:text-foreground">
                    <Calendar aria-hidden className="size-[1.1em]" />
                    No date
                  </span>
                )}
            </Button>
          }
        />
        <PopoverContent align="end" className="w-auto gap-0 p-0">
          <DatePicker
            startValue={task.dueStartAt ?? null}
            value={task.dueAt}
            onClear={() => setDueDate(null, null)}
            onDone={({ start, end }) => setDueDate(start, end)}
          />
        </PopoverContent>
      </Popover>
    </div>
  )
}
