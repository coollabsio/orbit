import { useId, useState } from 'react'
import { Add as Plus, Signpost, Tag, User as UserIcon, Xmark as X } from 'reicon-react'
import type { LabelRecord, TaskPayload } from '@/api/generated/types.gen'
import { Modal } from '@/components/common/Modal'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import type { RecurringTask, Template } from '@/features/tasks/api/intake'
import type { Milestone } from '@/features/tasks/api/milestones'
import type { TaskPriority, TaskStatusDef } from '@/features/tasks/api/models'
import { dayInputToIso, isoToDayInput } from '@/features/tasks/roadmap/roadmapLib'
import { INTERVAL_UNITS, MODE_LABEL, parseEveryCount, type IntervalUnit, type RecurringMode } from '@/features/tasks/recurringLib'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import type { User } from '@/features/workspaces/models'
import { PriorityIcon } from './PriorityIcon'
import { LabelPill } from './TaskLabels'
import { TaskStatusIcon } from './TaskStatusIcon'

/** What the pickers of the editor offer: the statuses, milestones and people of the project. */
export interface PayloadOptions {
  /** The project's statuses a task can start in (no Duplicate), in workflow order. */
  statuses: TaskStatusDef[]
  milestones: Milestone[]
  members: User[]
  labels: LabelRecord[]
}

/** The editor's draft: every field present, sub-issues as titles. */
export interface PayloadDraft {
  title: string
  description: string
  statusId: string | null
  priority: TaskPriority
  labelIds: string[]
  assigneeIds: string[]
  milestoneId: string | null
  dueOffsetDays: string
  subIssues: string[]
}

export function draftFromPayload(payload?: TaskPayload): PayloadDraft {
  return {
    title: payload?.title ?? '',
    description: payload?.description ?? '',
    statusId: payload?.status_id ?? null,
    priority: (payload?.priority ?? 'none') as TaskPriority,
    labelIds: payload?.label_ids ?? [],
    assigneeIds: payload?.assignee_ids ?? [],
    milestoneId: payload?.milestone_id ?? null,
    dueOffsetDays: payload?.due_offset_days == null ? '' : String(payload.due_offset_days),
    subIssues: (payload?.sub_issues ?? []).map((item) => item.title),
  }
}

/** The stored payload of a draft; null while the draft cannot be saved (no title, a bad due offset). */
export function payloadFromDraft(draft: PayloadDraft): TaskPayload | null {
  const title = draft.title.trim()
  const offset = draft.dueOffsetDays.trim()
  if (!title || (offset !== '' && !/^\d{1,4}$/.test(offset))) return null
  return {
    title,
    description: draft.description.trim(),
    status_id: draft.statusId,
    priority: draft.priority,
    label_ids: draft.labelIds,
    assignee_ids: draft.assigneeIds,
    milestone_id: draft.milestoneId,
    due_offset_days: offset === '' ? null : Number(offset),
    sub_issues: draft.subIssues.map((item) => item.trim()).filter(Boolean).map((item) => ({ title: item })),
  }
}

const toggle = (ids: string[], id: string) => (ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id])

/** The task fields of a template or a recurring task. */
function PayloadFields({ draft, options, disabled, onChange }: { draft: PayloadDraft; options: PayloadOptions; disabled: boolean; onChange: (draft: PayloadDraft) => void }) {
  const titleId = useId()
  const descriptionId = useId()
  const dueId = useId()
  const set = (fields: Partial<PayloadDraft>) => onChange({ ...draft, ...fields })
  const status = options.statuses.find((item) => item.id === draft.statusId)
  const milestone = options.milestones.find((item) => item.id === draft.milestoneId)
  const assignees = options.members.filter((member) => draft.assigneeIds.includes(member.id))
  const labels = options.labels.filter((label) => draft.labelIds.includes(label.id))

  return (
    <>
      <Field>
        <FieldLabel htmlFor={titleId}>Task title <span className="font-semibold text-primary">*</span></FieldLabel>
        <Input id={titleId} required maxLength={500} value={draft.title} disabled={disabled} onChange={(event) => set({ title: event.target.value })} />
      </Field>
      <Field>
        <FieldLabel htmlFor={descriptionId}>Description</FieldLabel>
        <Textarea id={descriptionId} rows={3} placeholder="Markdown is supported." value={draft.description} disabled={disabled} onChange={(event) => set({ description: event.target.value })} />
      </Field>
      <div className="flex flex-wrap items-center gap-1.5">
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="outline" size="sm" className="font-normal" disabled={disabled} aria-label={`Status: ${status?.name ?? 'default'}`}>
                <TaskStatusIcon status={status} />
                {status?.name ?? 'Default status'}
              </Button>
            }
          />
          <DropdownMenuContent className="w-auto min-w-45">
            <DropdownMenuItem onClick={() => set({ statusId: null })}>Default status</DropdownMenuItem>
            {options.statuses.map((option) => (
              <DropdownMenuItem key={option.id} className="data-selected:bg-accent data-selected:font-medium" data-selected={option.id === draft.statusId || undefined} onClick={() => set({ statusId: option.id })}>
                <TaskStatusIcon status={option} />
                {option.name}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="outline" size="sm" className="font-normal" disabled={disabled} aria-label={`Priority: ${PRIORITY_LABEL[draft.priority]}`}>
                <PriorityIcon priority={draft.priority} />
                {draft.priority === 'none' ? 'Priority' : PRIORITY_LABEL[draft.priority]}
              </Button>
            }
          />
          <DropdownMenuContent className="w-auto min-w-45">
            {PRIORITY_ORDER.map((priority) => (
              <DropdownMenuItem key={priority} className="data-selected:bg-accent data-selected:font-medium" data-selected={priority === draft.priority || undefined} onClick={() => set({ priority })}>
                <PriorityIcon priority={priority} />
                {PRIORITY_LABEL[priority]}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="outline" size="sm" className="font-normal" disabled={disabled} aria-label={`Assignees: ${assignees.length}`}>
                <UserIcon aria-hidden />
                {assignees.length === 0 ? 'Assignee' : assignees.length === 1 ? assignees[0]!.name : `${assignees.length} assignees`}
              </Button>
            }
          />
          <DropdownMenuContent className="max-h-72 w-auto min-w-52">
            {options.members.map((member) => (
              <DropdownMenuCheckboxItem key={member.id} checked={draft.assigneeIds.includes(member.id)} onCheckedChange={() => set({ assigneeIds: toggle(draft.assigneeIds, member.id) })}>
                <UserAvatar user={member} size={16} />
                {member.name}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        {options.labels.length > 0 ? (
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button type="button" variant="outline" size="sm" className="font-normal" disabled={disabled} aria-label={`Labels: ${labels.length}`}>
                  <Tag aria-hidden />
                  {labels.length === 0 ? 'Labels' : labels.length === 1 ? labels[0]!.name : `${labels.length} labels`}
                </Button>
              }
            />
            <DropdownMenuContent className="max-h-72 w-auto min-w-45">
              {options.labels.map((label) => (
                <DropdownMenuCheckboxItem key={label.id} checked={draft.labelIds.includes(label.id)} onCheckedChange={() => set({ labelIds: toggle(draft.labelIds, label.id) })}>
                  <LabelPill label={label} />
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
        {options.milestones.length > 0 ? (
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button type="button" variant="outline" size="sm" className="max-w-48 font-normal" disabled={disabled} aria-label={`Milestone: ${milestone?.name ?? 'none'}`}>
                  <Signpost aria-hidden />
                  <span className="truncate">{milestone?.name ?? 'Milestone'}</span>
                </Button>
              }
            />
            <DropdownMenuContent className="w-auto min-w-45">
              <DropdownMenuItem onClick={() => set({ milestoneId: null })}>No milestone</DropdownMenuItem>
              {options.milestones.map((option) => (
                <DropdownMenuItem key={option.id} className="data-selected:bg-accent data-selected:font-medium" data-selected={option.id === draft.milestoneId || undefined} onClick={() => set({ milestoneId: option.id })}>
                  {option.name}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
      </div>
      <Field>
        <FieldLabel htmlFor={dueId}>Due, in days after the task is created</FieldLabel>
        <Input id={dueId} inputMode="numeric" className="w-28" placeholder="No due date" value={draft.dueOffsetDays} disabled={disabled} onChange={(event) => set({ dueOffsetDays: event.target.value })} />
      </Field>
      <fieldset className="flex flex-col gap-1.5">
        <legend className="mb-1.5 text-sm font-medium">Sub-issues</legend>
        {draft.subIssues.map((title, index) => (
          // eslint-disable-next-line react/no-array-index-key -- the rows are positions of a draft list; a title is not unique
          <div key={index} className="flex items-center gap-1.5">
            <Input aria-label={`Sub-issue ${index + 1}`} maxLength={500} value={title} disabled={disabled} onChange={(event) => set({ subIssues: draft.subIssues.map((item, at) => (at === index ? event.target.value : item)) })} />
            <Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={`Remove sub-issue ${index + 1}`} disabled={disabled} onClick={() => set({ subIssues: draft.subIssues.filter((_, at) => at !== index) })}>
              <X />
            </Button>
          </div>
        ))}
        <Button type="button" variant="ghost" size="sm" className="self-start" disabled={disabled || draft.subIssues.length >= 50} onClick={() => set({ subIssues: [...draft.subIssues, ''] })}>
          <Plus aria-hidden />
          Add sub-issue
        </Button>
      </fieldset>
    </>
  )
}

/** Create or edit a template: a name and the task fields. */
export function TemplateModal({ template, options, busy, failed, onClose, onSave }: {
  template?: Template
  options: PayloadOptions
  busy: boolean
  failed: boolean
  onClose: () => void
  onSave: (name: string, payload: TaskPayload) => void
}) {
  const [name, setName] = useState(template?.name ?? '')
  const [draft, setDraft] = useState(() => draftFromPayload(template?.payload))
  const nameId = useId()
  const payload = payloadFromDraft(draft)
  return (
    <Modal title={template ? 'Edit template' : 'New template'} onClose={onClose} className="sm:max-w-lg">
      <form
        className="grid gap-4"
        onSubmit={(event) => {
          event.preventDefault()
          if (name.trim() && payload && !busy) onSave(name.trim(), payload)
        }}
      >
        <Field>
          <FieldLabel htmlFor={nameId}>Template name <span className="font-semibold text-primary">*</span></FieldLabel>
          <Input id={nameId} autoFocus required maxLength={200} placeholder="Bug report" value={name} disabled={busy} onChange={(event) => setName(event.target.value)} />
        </Field>
        <PayloadFields draft={draft} options={options} disabled={busy} onChange={setDraft} />
        {failed ? <p role="alert" className="text-xs text-destructive">The template was not saved. A template name must be unique in the project.</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={busy} />}>Cancel</DialogClose>
          <Button type="submit" disabled={!name.trim() || !payload || busy}>{busy ? 'Saving…' : 'Save template'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

export interface RecurringDraft {
  payload: TaskPayload
  mode: RecurringMode
  every_count: number
  every_unit: IntervalUnit
  /** Set only when the person changed the start day. */
  starts_at?: string
}

/** Create or edit a recurring task: the task fields, the mode and the interval. */
export function RecurringTaskModal({ routine, options, busy, failed, onClose, onSave }: {
  routine?: RecurringTask
  options: PayloadOptions
  busy: boolean
  failed: boolean
  onClose: () => void
  onSave: (values: RecurringDraft) => void
}) {
  const [draft, setDraft] = useState(() => draftFromPayload(routine?.payload))
  const [mode, setMode] = useState<RecurringMode>(routine?.mode === 'after_completion' ? 'after_completion' : 'schedule')
  const [count, setCount] = useState(String(routine?.every_count ?? 1))
  const [unit, setUnit] = useState<IntervalUnit>(INTERVAL_UNITS.find((item) => item === routine?.every_unit) ?? 'week')
  const savedStart = isoToDayInput(routine?.anchor_at)
  const [start, setStart] = useState(savedStart)
  const countId = useId()
  const startId = useId()
  const payload = payloadFromDraft(draft)
  const everyCount = parseEveryCount(count)
  const startsAt = start !== savedStart ? dayInputToIso(start) : null
  return (
    <Modal title={routine ? 'Edit recurring task' : 'New recurring task'} onClose={onClose} className="sm:max-w-lg">
      <form
        className="grid gap-4"
        onSubmit={(event) => {
          event.preventDefault()
          if (payload && everyCount && !busy) onSave({ payload, mode, every_count: everyCount, every_unit: unit, ...(startsAt ? { starts_at: startsAt } : {}) })
        }}
      >
        <PayloadFields draft={draft} options={options} disabled={busy} onChange={setDraft} />
        <div className="flex flex-wrap items-end gap-3 border-t pt-4">
          <Field className="w-auto">
            <FieldLabel>Repeat</FieldLabel>
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button type="button" variant="outline" size="sm" className="font-normal" disabled={busy}>{MODE_LABEL[mode]}</Button>} />
              <DropdownMenuContent className="w-auto min-w-44">
                {(Object.keys(MODE_LABEL) as RecurringMode[]).map((value) => (
                  <DropdownMenuItem key={value} className="data-selected:bg-accent data-selected:font-medium" data-selected={value === mode || undefined} onClick={() => setMode(value)}>{MODE_LABEL[value]}</DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </Field>
          <Field className="w-auto">
            <FieldLabel htmlFor={countId}>Every</FieldLabel>
            <Input id={countId} inputMode="numeric" className="w-16" value={count} disabled={busy} aria-invalid={everyCount === null || undefined} onChange={(event) => setCount(event.target.value)} />
          </Field>
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button type="button" variant="outline" size="sm" className="font-normal" disabled={busy} aria-label={`Unit: ${unit}`}>{everyCount === 1 ? unit : `${unit}s`}</Button>} />
            <DropdownMenuContent className="w-auto min-w-28">
              {INTERVAL_UNITS.map((value) => (
                <DropdownMenuItem key={value} className="data-selected:bg-accent data-selected:font-medium" data-selected={value === unit || undefined} onClick={() => setUnit(value)}>{everyCount === 1 ? value : `${value}s`}</DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <Field className="w-auto">
            <FieldLabel htmlFor={startId}>{mode === 'schedule' ? 'Starts on' : 'First task on'}</FieldLabel>
            <Input id={startId} type="date" className="w-40" value={start} disabled={busy} onChange={(event) => setStart(event.target.value)} />
          </Field>
        </div>
        <p className="text-xs text-muted-foreground">
          {mode === 'schedule'
            ? 'A task is created at each interval from the start day, also when the task before it is still open.'
            : 'The next task is created one interval after the task before it is closed.'}
        </p>
        {failed ? <p role="alert" className="text-xs text-destructive">The recurring task was not saved. Try again.</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={busy} />}>Cancel</DialogClose>
          <Button type="submit" disabled={!payload || !everyCount || busy}>{busy ? 'Saving…' : 'Save'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
