import { useState } from 'react'
import { DatePicker, type DueRange } from '@/components/common/DatePicker'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { Task } from '@/features/tasks/api/models'

/** The date picker in a dialog, for the due date of one or more tasks. It starts from their shared due range, or
 *  empty when they disagree. There is no Done step: picks collect while the dialog is open (to extend a day to a range
 *  or change the time) and `onPick` gets the last one when it closes, so a selection of many tasks is written once;
 *  Clear sends `null, null` at once. The caller closes the dialog from `onPick`; `onClose` alone means nothing changed. */
export function DueDateDialog({ tasks, onPick, onClose }: { tasks: Task[]; onPick: (start: string | null, end: string | null) => void; onClose: () => void }) {
  const [first] = tasks
  const sameDue = tasks.every((task) => task.dueAt === first.dueAt && (task.dueStartAt ?? null) === (first.dueStartAt ?? null))
  const [picked, setPicked] = useState<DueRange | null>(null)
  return (
    <Dialog open onOpenChange={(next) => {
      if (next) return
      if (picked) onPick(picked.start, picked.end)
      else onClose()
    }}>
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
          onChange={setPicked}
        />
      </DialogContent>
    </Dialog>
  )
}
