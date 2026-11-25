import { useState, type FormEvent } from 'react'
import { Add as Plus, Clock } from 'reicon-react'
import { cn } from 'cn'
import { formatDuration } from '@orbit/api/duration'
import type { TaskField } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Checkbox } from '@/components/ui/checkbox'
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useProjectTaskFields, useSetTaskFieldValue, useTaskFieldValues } from '@/features/tasks/api/fields'
import { useTaskTimeEntries } from '@/features/tasks/api/timeEntries'
import { fieldValueText, parseFieldInput, selectedOptions } from '@/features/tasks/fieldsLib'
import { PropertyButton, PropertyGroup } from './TaskPropertyParts'

type Save = (value: unknown) => void

/** One property block per custom field of the task's project, in the project's order. */
export function TaskFieldProperties({ workspaceId, taskId, projectId, readOnly, onLogTime }: { workspaceId: string; taskId: string; projectId: string; readOnly: boolean; onLogTime: () => void }) {
  const fields = useProjectTaskFields(workspaceId, projectId).data ?? []
  const values = useTaskFieldValues(workspaceId, taskId).data ?? []
  const setValue = useSetTaskFieldValue(workspaceId, taskId)
  if (fields.length === 0) return null
  return (
    <>
      {fields.map((field) => {
        if (field.kind === 'time_tracking') {
          return (
            <PropertyGroup key={field.id} title={field.name}>
              <TimeProperty workspaceId={workspaceId} taskId={taskId} readOnly={readOnly} onLogTime={onLogTime} />
            </PropertyGroup>
          )
        }
        const value = values.find((item) => item.field_id === field.id)?.value ?? null
        const save: Save = (next) => setValue.mutate({ fieldId: field.id, value: next })
        return (
          <PropertyGroup key={field.id} title={field.name}>
            <FieldEditor field={field} value={value} readOnly={readOnly} onSave={save} />
          </PropertyGroup>
        )
      })}
      {setValue.isError ? <p className="-mt-4 mb-6 text-xs text-destructive" role="alert">The field could not be saved.</p> : null}
    </>
  )
}

/** The time logged on the task and its sub-issues, and Log time. */
function TimeProperty({ workspaceId, taskId, readOnly, onLogTime }: { workspaceId: string; taskId: string; readOnly: boolean; onLogTime: () => void }) {
  const time = useTaskTimeEntries(workspaceId, taskId).data
  return (
    <>
      {time && (time.total_minutes > 0 || time.sub_issues_minutes > 0) ? (
        <p className="flex items-center gap-1.5 text-[0.8125rem] tabular-nums" aria-label="Time logged">
          <Clock className="size-3.5 text-muted-foreground" aria-hidden="true" />
          {formatDuration(time.total_minutes)}
          {time.sub_issues_minutes > 0 ? <span className="text-xs text-muted-foreground">+{formatDuration(time.sub_issues_minutes)} sub-issues</span> : null}
        </p>
      ) : null}
      <PropertyButton type="button" className="text-muted-foreground" disabled={readOnly} onClick={onLogTime}>
        <Plus className="size-3.5" aria-hidden="true" />
        Log time
      </PropertyButton>
    </>
  )
}

function FieldEditor({ field, value, readOnly, onSave }: { field: TaskField; value: unknown; readOnly: boolean; onSave: Save }) {
  const text = fieldValueText(field, value)
  const empty = !text
  const label = `${field.name}: ${text || 'none'}`
  switch (field.kind) {
    case 'checkbox':
      return (
        <label className="flex h-8 items-center gap-2 text-[0.8125rem]">
          <Checkbox aria-label={field.name} checked={value === true} disabled={readOnly} onCheckedChange={(checked) => onSave(checked === true)} />
          {value === true ? 'Yes' : <span className="text-muted-foreground">No</span>}
        </label>
      )
    case 'select':
    case 'multi_select': {
      const chosen = selectedOptions(field, value)
      const multi = field.kind === 'multi_select'
      const toggle = (id: string) => {
        if (!multi) return onSave(chosen[0]?.id === id ? null : id)
        const ids = chosen.map((option) => option.id)
        onSave(ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id])
      }
      return (
        <DropdownMenu>
          <DropdownMenuTrigger disabled={readOnly} render={
            <PropertyButton className={cn('h-auto min-h-8 max-w-full flex-wrap py-1', empty && 'text-muted-foreground')} aria-label={label}>
              {chosen.length === 0 ? `Set ${field.name}` : chosen.map((option) => (
                <span key={option.id} className="inline-flex items-center gap-1"><ColorDot color={option.color} />{option.name}</span>
              ))}
            </PropertyButton>
          } />
          <DropdownMenuContent align="start" className="w-auto min-w-45">
            <DropdownMenuGroup>
              {field.options.length === 0 ? <DropdownMenuItem disabled>No options yet</DropdownMenuItem> : null}
              {field.options.map((option) => (
                <DropdownMenuCheckboxItem key={option.id} checked={chosen.some((item) => item.id === option.id)} closeOnClick={!multi} onCheckedChange={() => toggle(option.id)}>
                  <ColorDot color={option.color} />
                  {option.name}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuGroup>
            {chosen.length > 0 ? (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => onSave(null)}>Clear</DropdownMenuItem>
              </>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
      )
    }
    case 'date':
      return <DateEditor field={field} value={typeof value === 'string' ? value : null} text={text} label={label} readOnly={readOnly} onSave={onSave} />
    default:
      return <TextEditor field={field} value={value} text={text} label={label} readOnly={readOnly} onSave={onSave} />
  }
}

/** A text, number or URL value: a popover with one input; Enter saves, an empty input clears. */
function TextEditor({ field, value, text, label, readOnly, onSave }: { field: TaskField; value: unknown; text: string; label: string; readOnly: boolean; onSave: Save }) {
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState('')
  const [invalid, setInvalid] = useState(false)
  const submit = (event: FormEvent) => {
    event.preventDefault()
    const next = parseFieldInput(field.kind, draft)
    if (next === undefined) return setInvalid(true)
    if (next !== value) onSave(next)
    setOpen(false)
  }
  return (
    <Popover open={open} onOpenChange={(next) => {
      if (next) {
        setDraft(value === null || value === undefined ? '' : String(value))
        setInvalid(false)
      }
      setOpen(next)
    }}>
      <PopoverTrigger disabled={readOnly} render={
        <PropertyButton className={cn('max-w-full', !text && 'text-muted-foreground')} aria-label={label}>
          <span className="truncate">{text || `Set ${field.name}`}</span>
        </PropertyButton>
      } />
      <PopoverContent align="start" className="w-72 p-2">
        <form className="flex flex-col gap-2" onSubmit={submit}>
          <Input
            autoFocus
            aria-label={field.name}
            aria-invalid={invalid || undefined}
            inputMode={field.kind === 'number' ? 'decimal' : undefined}
            type={field.kind === 'url' ? 'url' : 'text'}
            placeholder={field.kind === 'number' ? '0' : field.kind === 'url' ? 'https://' : field.name}
            maxLength={field.kind === 'url' ? 2048 : 2000}
            value={draft}
            onChange={(event) => { setDraft(event.target.value); setInvalid(false) }}
          />
          {invalid ? <p className="m-0 text-xs text-destructive" role="alert">{field.kind === 'number' ? 'Enter a number.' : 'Enter an http or https link.'}</p> : null}
          <div className="flex justify-end gap-2">
            {field.kind === 'url' && typeof value === 'string' ? (
              <a className="mr-auto self-center text-xs text-muted-foreground underline" href={value} target="_blank" rel="noopener noreferrer">Open link</a>
            ) : null}
            <Button type="button" variant="ghost" size="sm" onClick={() => setOpen(false)}>Cancel</Button>
            <Button type="submit" size="sm">Save</Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  )
}

function dayOf(value: string): Date {
  const [year, month, day] = value.split('-').map(Number)
  return new Date(year!, month! - 1, day!)
}

function dayValue(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

function DateEditor({ field, value, text, label, readOnly, onSave }: { field: TaskField; value: string | null; text: string; label: string; readOnly: boolean; onSave: Save }) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger disabled={readOnly} render={
        <PropertyButton className={cn(!text && 'text-muted-foreground')} aria-label={label}>
          {text || `Set ${field.name}`}
        </PropertyButton>
      } />
      <PopoverContent align="start" className="w-auto gap-0 p-0">
        <Calendar
          mode="single"
          selected={value ? dayOf(value) : undefined}
          defaultMonth={value ? dayOf(value) : undefined}
          onSelect={(date) => {
            onSave(date ? dayValue(date) : null)
            setOpen(false)
          }}
        />
        {value ? (
          <div className="border-t p-2">
            <Button variant="ghost" size="sm" className="w-full" onClick={() => { onSave(null); setOpen(false) }}>Clear</Button>
          </div>
        ) : null}
      </PopoverContent>
    </Popover>
  )
}
