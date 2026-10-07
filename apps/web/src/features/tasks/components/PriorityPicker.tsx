import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { PriorityIcon } from './PriorityIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import type { Task } from '@/features/tasks/api/models'
import { useTaskPickerUpdate } from '@/features/tasks/useTaskPickerUpdate'

/** Priority glyph that opens a menu to change the priority in place (list rows, board cards, timeline and sub-issue rows). */
export function PriorityPicker({ task, align = 'left', className }: { task: Pick<Task, 'id' | 'version' | 'priority'>; align?: 'left' | 'right'; className?: string }) {
  const save = useTaskPickerUpdate(task, 'Priority update failed.')
  return (
    <div className={className} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DropdownMenu modal={false}>
        <Tip label="Change priority">
          <DropdownMenuTrigger
            render={
              <Button variant="ghost" size="icon-sm" className="size-[22px] text-muted-foreground/70" aria-label={`Priority: ${PRIORITY_LABEL[task.priority]}`}>
                <PriorityIcon priority={task.priority} />
              </Button>
            }
          />
        </Tip>
        <DropdownMenuContent align={align === 'right' ? 'end' : 'start'} className="w-auto min-w-45">
          {PRIORITY_ORDER.map((priority) => (
            <DropdownMenuItem
              key={priority}
              className="data-selected:bg-accent data-selected:font-medium"
              data-selected={priority === task.priority || undefined}
              onClick={() => { if (priority !== task.priority) save({ priority }) }}
            >
              <PriorityIcon priority={priority} />
              {PRIORITY_LABEL[priority]}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}
