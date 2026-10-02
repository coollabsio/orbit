import { DatePicker } from '@/components/common/DatePicker'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { Task } from '@/features/tasks/api/models'

/** The date picker in a dialog, for the due date of one or more tasks. It starts from their shared due range, or
 *  empty when they disagree. `onPick` gets the new range (`null, null` clears it); the caller closes the dialog. */
export function DueDateDialog({ tasks, onPick, onClose }: { tasks: Task[]; onPick: (start: string | null, end: string | null) => void; onClose: () => void }) {
  const [first] = tasks
  const sameDue = tasks.every((task) => task.dueAt === first.dueAt && (task.dueStartAt ?? null) === (first.dueStartAt ?? null))
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose() }}>
      <DialogContent showCloseButton={false} className="top-1/3 w-auto translate-y-0 gap-0 overflow-hidden p-0 duration-150 data-open:zoom-in-97! data-closed:zoom-out-97! sm:max-w-none">
        <DialogHeader className="sr-only">
          <DialogTitle>Set due date</DialogTitle>
          <DialogDescription>{tasks.length === 1 ? first.identifier : `${tasks.length} tasks`}</DialogDescription>
        </DialogHeader>
        <DatePicker
          startValue={sameDue ? first.dueStartAt ?? null : null}
          value={sameDue ? first.dueAt : null}
          clearable={tasks.some((task) => task.dueAt)}
          onClear={() => onPick(null, null)}
          onDone={({ start, end }) => onPick(start, end)}
        />
      </DialogContent>
    </Dialog>
  )
}
