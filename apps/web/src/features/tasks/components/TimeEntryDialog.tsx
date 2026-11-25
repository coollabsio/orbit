import { useId, useState } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldError, FieldLabel } from '@/components/ui/field'
import { Modal } from '@/components/common/Modal'
import { InfoTip } from '@/components/common/InfoTip'
import { SearchPicker } from '@/components/common/SearchPicker'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { User } from '@/features/workspaces/models'
import type { Task, TimeEntry } from '@/features/tasks/api/models'
import { useProjectAudience, useProjects } from '@/features/tasks/api/projects'
import { useCanIn } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCreateTimeEntry, useUpdateTimeEntry } from '@/features/tasks/api/timeEntries'
import { formatDuration, parseDuration } from '@orbit/api/duration'
import { todayLocalDate } from '@/features/tasks/timeLib'
import { personOption } from './pickerOptions'

interface TimeEntryDialogProps {
  task: Pick<Task, 'id' | 'projectId'>
  /** The workspace's people; the person picker offers those who see the task's project. */
  users: User[]
  currentUserId: string
  /** Edit this entry; without it the dialog logs a new one. */
  entry?: TimeEntry
  onClose: () => void
}

/** Logs time on a task, or edits a logged entry. With `time.manage` in the project, the time can be another member's. */
export function TimeEntryDialog({ task, users, currentUserId, entry, onClose }: TimeEntryDialogProps) {
  const { workspace } = useWorkspace()
  const canManage = useCanIn('time.manage', task.projectId)
  const project = useProjects(workspace.id).data?.find((item) => item.id === task.projectId)
  const members = useProjectAudience(workspace.id, project, users).filter((user) => !user.suspended || user.id === entry?.user_id)
  const createEntry = useCreateTimeEntry(workspace.id, task.id)
  const updateEntry = useUpdateTimeEntry(workspace.id)
  const [duration, setDuration] = useState(entry ? formatDuration(entry.minutes) : '')
  const [durationTouched, setDurationTouched] = useState(false)
  const [workedOn, setWorkedOn] = useState(entry?.worked_on ?? todayLocalDate())
  const [note, setNote] = useState(entry?.note ?? '')
  const [billable, setBillable] = useState(entry?.billable ?? false)
  const [userId, setUserId] = useState(entry?.user_id ?? currentUserId)
  const ids = { duration: useId(), date: useId(), note: useId(), billable: useId() }
  const minutes = parseDuration(duration)
  const durationError = durationTouched && minutes === null ? 'Enter a time such as 1h 30m, up to 24h.' : null
  const pending = createEntry.isPending || updateEntry.isPending
  const failure = createEntry.error ?? updateEntry.error
  const person = members.find((member) => member.id === userId)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setDurationTouched(true)
    if (minutes === null || !workedOn || pending) return
    const input = { minutes, worked_on: workedOn, note, billable, ...(canManage ? { user_id: userId } : {}) }
    try {
      if (entry) await updateEntry.mutateAsync({ entryId: entry.id, version: entry.version, changes: input })
      else await createEntry.mutateAsync(input)
      onClose()
    } catch {
      // the error shows under the form; the draft stays for a retry
    }
  }

  return (
    <Modal title={entry ? 'Edit time' : 'Log time'} onClose={onClose} className="sm:max-w-md" dismissible={!pending}>
      <form className="grid gap-4" onSubmit={(event) => void submit(event)}>
        <div className="grid grid-cols-2 items-start gap-3">
          <Field data-invalid={durationError ? true : undefined}>
            <FieldLabel htmlFor={ids.duration} className="items-center gap-1.5">Duration <InfoTip text="Hours and minutes, such as 1h 30m, 90m or 1.5h. A bare number is minutes." /></FieldLabel>
            <Input
              id={ids.duration}
              autoFocus
              placeholder="1h 30m"
              value={duration}
              aria-invalid={durationError ? true : undefined}
              onChange={(event) => setDuration(event.target.value)}
              onBlur={() => { if (duration.trim()) setDurationTouched(true) }}
            />
            {durationError ? <FieldError>{durationError}</FieldError> : null}
          </Field>
          <Field>
            <FieldLabel htmlFor={ids.date}>Date</FieldLabel>
            <Input id={ids.date} type="date" required value={workedOn} onChange={(event) => setWorkedOn(event.target.value)} />
          </Field>
        </div>
        {canManage ? (
          <Field>
            <FieldLabel>Person</FieldLabel>
            <SearchPicker
              label="Person"
              placeholder="Log time for…"
              emptyText="No people match"
              options={members.map(personOption)}
              selected={[userId]}
              onSelect={setUserId}
              trigger={
                <Button type="button" variant="outline" className="justify-start" aria-label={`Person: ${person?.name ?? 'Me'}`}>
                  <UserAvatar user={person} size={16} />
                  <span className="truncate">{person?.name ?? 'Me'}</span>
                </Button>
              }
            />
          </Field>
        ) : null}
        <Field>
          <FieldLabel htmlFor={ids.note} className="items-center gap-1.5">Note <InfoTip text="What you worked on. Markdown works." /></FieldLabel>
          <Textarea id={ids.note} rows={3} maxLength={10000} value={note} onChange={(event) => setNote(event.target.value)} />
        </Field>
        <label htmlFor={ids.billable} className="flex items-center gap-2 text-sm">
          <Switch id={ids.billable} checked={billable} onCheckedChange={(checked: boolean) => setBillable(checked)} />
          Billable
        </label>
        {failure ? <FieldError>{failure instanceof ApiProblem ? failure.detail : 'The time could not be saved. Try again.'}</FieldError> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={pending} />}>Cancel</DialogClose>
          <Button type="submit" disabled={pending}>{entry ? 'Save' : 'Log time'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
