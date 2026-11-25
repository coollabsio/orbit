import { useState, type FormEvent } from 'react'
import { Xmark as X, Plus } from 'reicon-react'
import type { TaskField, TaskFieldBody, TaskFieldKind, TaskFieldOptionInput } from '@orbit/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { LabelColorPicker } from '@/features/tasks/components/TaskLabels'
import { FIELD_KINDS, hasOptions, newFieldKinds, nextOptionColor } from '@/features/tasks/fieldsLib'

/**
 * Name, kind and (for select kinds) options of a gallery field. An existing field keeps its kind: the stored values
 * would not fit another one. A new field cannot be a second time tracking field of the `gallery`.
 */
function TaskFieldForm({ initial, gallery = [], submitLabel, pending, onSubmit }: { initial?: TaskField; gallery?: readonly TaskField[]; submitLabel: string; pending: boolean; onSubmit: (body: TaskFieldBody) => Promise<void> }) {
  const kinds = initial ? FIELD_KINDS : newFieldKinds(gallery)
  const [name, setName] = useState(initial?.name ?? '')
  const [kind, setKind] = useState<TaskFieldKind>(initial?.kind ?? 'text')
  const [options, setOptions] = useState<TaskFieldOptionInput[]>(initial?.options ?? [])
  const [colorOf, setColorOf] = useState<number | null>(null)
  const names = options.map((option) => option.name.trim().toLowerCase())
  const optionsValid = !hasOptions(kind) || names.every((value, index) => value && names.indexOf(value) === index)
  const valid = Boolean(name.trim()) && optionsValid

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!valid || pending) return
    try {
      await onSubmit({ name: name.trim(), kind, options: hasOptions(kind) ? options.map((option) => ({ ...option, name: option.name.trim() })) : [] })
    } catch {
      // The dialog shows the mutation error.
    }
  }
  const change = (index: number, patch: Partial<TaskFieldOptionInput>) => setOptions(options.map((option, at) => (at === index ? { ...option, ...patch } : option)))

  return (
    <form className="flex w-full flex-col gap-3" onSubmit={(event) => void submit(event)}>
      <div className="grid grid-cols-[minmax(0,1fr)_10rem] items-center gap-3">
        <Input className="w-full" aria-label="Field name" placeholder="Field name, e.g. Customer" maxLength={100} value={name} onChange={(event) => setName(event.target.value)} autoFocus />
        <Select items={kinds.map((item) => ({ value: item.kind, label: item.label }))} value={kind} disabled={Boolean(initial)} onValueChange={(value) => value && setKind(value as TaskFieldKind)}>
          <SelectTrigger aria-label="Field type" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {kinds.map((item) => <SelectItem key={item.kind} value={item.kind}>{item.label}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      {hasOptions(kind) ? (
        <div className="flex flex-col gap-2">
          {options.map((option, index) => (
            <div key={option.id ?? `new-${index}`} className="flex flex-col gap-2">
              <div className="flex items-center gap-2">
                <button type="button" aria-label={`Color of ${option.name || 'option'}`} aria-expanded={colorOf === index} className="size-4 shrink-0 rounded-full outline-none focus-visible:ring-3 focus-visible:ring-ring/50" style={{ backgroundColor: option.color }} onClick={() => setColorOf(colorOf === index ? null : index)} />
                <Input className="h-8 min-w-0 flex-1" aria-label={`Option ${index + 1}`} placeholder="Option name" maxLength={100} value={option.name} onChange={(event) => change(index, { name: event.target.value })} />
                <Button type="button" variant="ghost" size="icon-sm" aria-label={`Remove ${option.name || 'option'}`} onClick={() => setOptions(options.filter((_, at) => at !== index))}>
                  <X className="size-3.5" />
                </Button>
              </div>
              {colorOf === index ? <div className="pl-6"><LabelColorPicker value={option.color} onChange={(color) => { change(index, { color }); setColorOf(null) }} /></div> : null}
            </div>
          ))}
          <Button type="button" variant="ghost" className="-ml-2 self-start text-muted-foreground" disabled={options.length >= 100} onClick={() => setOptions([...options, { name: '', color: nextOptionColor(options) }])}>
            <Plus className="size-3.5" />
            Add option
          </Button>
          {!optionsValid ? <p className="m-0 text-xs text-destructive" role="alert">Each option needs a name, and no two options can have the same name.</p> : null}
        </div>
      ) : null}
      <div className="flex justify-end">
        <Button type="submit" disabled={!valid || pending}>{submitLabel}</Button>
      </div>
    </form>
  )
}

/** The create or edit dialog of a gallery field; `error` shows under the form. */
export function TaskFieldDialog({ title, error, onClose, ...form }: Parameters<typeof TaskFieldForm>[0] & { title: string; error?: string; onClose: () => void }) {
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <TaskFieldForm {...form} />
        {error ? <p className="m-0 text-xs text-destructive" role="alert">{error}</p> : null}
      </DialogContent>
    </Dialog>
  )
}
