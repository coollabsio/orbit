import { useEffect, useRef, useState } from 'react'
import { Calendar } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DatePicker } from '@/components/common/DatePicker'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useTaskPickerUpdate } from '@/features/tasks/useTaskPickerUpdate'
import { DueDateChip } from './TaskPropertyChips'

/**
 * Due-date chip that opens the date picker in place (list rows and board cards). Every pick saves at once and the
 * picker stays open (to extend a day to a range or change the time); Clear saves and closes. Without a date it shows
 * a quiet "No date" placeholder, or only a calendar glyph (`empty="icon"`). Font size comes from `className`.
 * `onOpenChange` tells the caller when the picker opens and closes (also when it unmounts while open), so a caller
 * that places the chip by whether the task has a date can keep this one mounted until the picking is over.
 */
export function DueDatePicker({ task, status, empty = 'label', className, onOpenChange }: {
  task: Task
  status: TaskStatusDef | undefined
  empty?: 'label' | 'icon'
  className?: string
  onOpenChange?: (open: boolean) => void
}) {
  const save = useTaskPickerUpdate(task, 'Due date update failed.')
  const [open, setOpenState] = useState(false)
  const notify = useRef(onOpenChange)
  useEffect(() => {
    notify.current = onOpenChange
  })
  useEffect(() => () => notify.current?.(false), [])
  const setOpen = (next: boolean) => {
    setOpenState(next)
    onOpenChange?.(next)
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
              {task.dueAt ? <DueDateChip task={task} status={status} className="hover:text-foreground" /> : null}
              {!task.dueAt && empty === 'icon' ? <Calendar aria-hidden className="size-3.5 text-muted-foreground/70 hover:text-foreground" /> : null}
              {!task.dueAt && empty === 'label' ? (
                <span className="inline-flex items-center gap-1 whitespace-nowrap text-muted-foreground/50 transition-colors group-hover/row:text-muted-foreground hover:text-foreground">
                  <Calendar aria-hidden className="size-[1.1em]" />
                  No date
                </span>
              ) : null}
            </Button>
          }
        />
        <PopoverContent align="end" className="w-auto gap-0 p-0">
          <DatePicker
            startValue={task.dueStartAt ?? null}
            value={task.dueAt}
            onClear={() => {
              save({ due_start_at: null, due_at: null })
              setOpen(false)
            }}
            onChange={({ start, end }) => save({ due_start_at: start, due_at: end })}
          />
        </PopoverContent>
      </Popover>
    </div>
  )
}
