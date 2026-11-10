import { useState } from 'react'
import { Add, Trash } from 'reicon-react'
import type { DatabaseButtonAction } from '@orbit/api/generated/types.gen'
import { BUTTON_ME, BUTTON_NOW, BUTTON_TODAY, MAX_BUTTON_ACTIONS, type ButtonConfig } from '@orbit/database/buttons'
import { isComputed } from '@orbit/database/meta'
import type { CellValue, Property, Row } from '@orbit/database/types'
import { apiValue, parseValue } from '@orbit/database/values'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Modal } from '@/components/common/Modal'
import { usePageTree } from '@/features/docs/api/pages'
import { pageTitle } from '@/features/docs/pageTree'
import { useDatabase, useUpdateProperty } from './api'
import { CellEditor } from './cells/CellEditor'
import { CellView } from './cells/CellView'
import { DatabaseEnvContext, useDatabaseEnv, type DatabaseEnv } from './context'
import { PROPERTY_TYPES } from './propertyMeta'

type ButtonProperty = Extract<Property, { type: 'button' }>
type Values = Record<string, unknown>

const ACTION_LABELS: Record<DatabaseButtonAction['type'], string> = {
  set_values: 'Edit this row',
  add_row: 'Add a row',
  open_url: 'Open a URL',
}

/** The button editor (spec P3 17.2): label, up to five actions run in order, and an optional confirmation. */
export function ButtonConfigDialog({ property, onClose }: { property: ButtonProperty; onClose: () => void }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const [draft, setDraft] = useState<ButtonConfig>(property.config)
  const [asks, setAsks] = useState(property.config.confirm !== null)
  const setAction = (index: number, action: DatabaseButtonAction | null) =>
    setDraft((current) => ({
      ...current,
      actions: action ? current.actions.map((other, at) => (at === index ? action : other)) : current.actions.filter((_, at) => at !== index),
    }))
  const addAction = (action: DatabaseButtonAction) => setDraft((current) => ({ ...current, actions: [...current.actions, action] }))
  const hasUrl = draft.actions.some((action) => action.type === 'open_url')
  const save = () => {
    const config: ButtonConfig = { ...draft, label: draft.label.trim(), confirm: asks && draft.confirm?.trim() ? draft.confirm.trim() : null }
    updateProperty.mutate({ propertyId: property.id, body: { config } }, { onSuccess: onClose })
  }

  return (
    <Modal title={`Button “${property.name}”`} description="A press runs the actions in order. If one fails, none of them is saved." onClose={onClose}>
      <div className="flex max-h-[65vh] flex-col gap-4 overflow-y-auto">
        <Field>
          <FieldLabel htmlFor="button-label">Label</FieldLabel>
          <Input id="button-label" value={draft.label} maxLength={200} placeholder={property.name} onChange={(event) => setDraft({ ...draft, label: event.target.value })} />
        </Field>
        <div className="flex flex-col gap-2">
          <span className="text-sm font-medium">Actions</span>
          {draft.actions.length === 0 ? <p className="text-sm text-muted-foreground">No actions yet.</p> : null}
          {draft.actions.map((action, index) => (
            <ActionEditor key={index} action={action} onChange={(next) => setAction(index, next)} onRemove={() => setAction(index, null)} />
          ))}
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button type="button" variant="outline" size="sm" className="self-start" disabled={draft.actions.length >= MAX_BUTTON_ACTIONS} />}>
              <Add />
              Add action
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuItem onClick={() => addAction({ type: 'set_values', values: {} })}>{ACTION_LABELS.set_values}</DropdownMenuItem>
              <DropdownMenuItem onClick={() => addAction({ type: 'add_row', database_id: databaseId, values: {} })}>{ACTION_LABELS.add_row}</DropdownMenuItem>
              <DropdownMenuItem disabled={hasUrl} onClick={() => addAction({ type: 'open_url', url: '' })}>
                {ACTION_LABELS.open_url}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="flex flex-col gap-2">
          <label className="flex items-center gap-2 text-sm">
            <Switch checked={asks} onCheckedChange={setAsks} />
            Ask before running
          </label>
          {asks ? (
            <Input
              aria-label="Confirmation question"
              value={draft.confirm ?? ''}
              maxLength={500}
              placeholder="Close this task?"
              onChange={(event) => setDraft({ ...draft, confirm: event.target.value })}
            />
          ) : null}
        </div>
      </div>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="button" disabled={updateProperty.isPending} onClick={save}>
          Save
        </Button>
      </div>
    </Modal>
  )
}

function ActionEditor({ action, onChange, onRemove }: { action: DatabaseButtonAction; onChange: (action: DatabaseButtonAction) => void; onRemove: () => void }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium">{ACTION_LABELS[action.type]}</span>
        <Button type="button" variant="ghost" size="icon-xs" aria-label="Remove action" onClick={onRemove}>
          <Trash />
        </Button>
      </div>
      {action.type === 'set_values' ? (
        <TargetValues workspaceId={workspaceId} databaseId={databaseId} adding={false} values={action.values} onChange={(values) => onChange({ ...action, values })} />
      ) : action.type === 'add_row' ? (
        <>
          <DatabasePicker value={action.database_id} onChange={(id) => onChange({ ...action, database_id: id, values: {} })} />
          <TargetValues workspaceId={workspaceId} databaseId={action.database_id} adding values={action.values ?? {}} onChange={(values) => onChange({ ...action, values })} />
        </>
      ) : (
        <Input aria-label="URL" type="url" placeholder="https://…" value={action.url} maxLength={2000} onChange={(event) => onChange({ ...action, url: event.target.value.trim() })} />
      )}
    </div>
  )
}

/** The databases of the workspace the viewer sees, this one first. */
function DatabasePicker({ value, onChange }: { value: string; onChange: (id: string) => void }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const tree = usePageTree(workspaceId).data ?? []
  const items = [
    { value: databaseId, label: 'This database' },
    ...tree.filter((page) => page.kind === 'database' && page.id !== databaseId).map((page) => ({ value: page.id, label: pageTitle(page) })),
  ]
  if (!items.some((item) => item.value === value)) items.push({ value, label: 'Another database' })
  return (
    <Select items={items} value={value} onValueChange={(next) => next && onChange(next as string)}>
      <SelectTrigger aria-label="Database" className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.value} value={item.value}>
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** The values an action writes, on the properties of `databaseId`; the title is offered only for a new row. */
function TargetValues({ workspaceId, databaseId, adding, values, onChange }: { workspaceId: string; databaseId: string; adding: boolean; values: Values; onChange: (values: Values) => void }) {
  const database = useDatabase(workspaceId, databaseId).data
  const env = useDatabaseEnv()
  if (!database) return <p className="text-sm text-muted-foreground">Loading properties…</p>
  const settable = database.properties.filter((property) => (property.type === 'title' ? adding : !isComputed(property.type) && property.type !== 'button'))
  const unused = settable.filter((property) => !(property.id in values))
  const set = (id: string, value: unknown) => onChange({ ...values, [id]: value })
  const remove = (id: string) => onChange(Object.fromEntries(Object.entries(values).filter(([key]) => key !== id)))
  return (
    <DatabaseEnvContext.Provider value={meEnv(env)}>
      <div className="flex flex-col gap-1.5">
        {settable
          .filter((property) => property.id in values)
          .map((property) => {
            const Icon = PROPERTY_TYPES[property.type].icon
            return (
              <div key={property.id} className="flex items-center gap-2 text-sm">
                <span className="flex w-36 shrink-0 items-center gap-1.5 truncate text-muted-foreground">
                  <Icon className="size-4 shrink-0" aria-hidden="true" />
                  <span className="truncate">{property.name}</span>
                </span>
                <div className="min-w-0 flex-1">
                  <ValueField property={property} raw={values[property.id]} onChange={(value) => set(property.id, value)} />
                </div>
                <Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${property.name}`} onClick={() => remove(property.id)}>
                  <Trash />
                </Button>
              </div>
            )
          })}
        {unused.length > 0 ? (
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="sm" className="self-start text-muted-foreground" />}>
              <Add />
              Set a property
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="max-h-72 overflow-y-auto">
              {unused.map((property) => {
                const Icon = PROPERTY_TYPES[property.type].icon
                return (
                  <DropdownMenuItem key={property.id} onClick={() => set(property.id, property.type === 'title' ? '' : null)}>
                    <Icon className="text-muted-foreground" />
                    {property.name}
                  </DropdownMenuItem>
                )
              })}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
      </div>
    </DatabaseEnvContext.Provider>
  )
}

/** People editors list "Me" (whoever presses the button) first, as templates do. */
function meEnv(env: DatabaseEnv): DatabaseEnv {
  const self = env.users.get(env.ctx.userId)
  if (!self) return env
  return {
    ...env,
    ctx: { ...env.ctx, members: new Map([[BUTTON_ME, 'Me'], ...env.ctx.members]) },
    users: new Map([[BUTTON_ME, { ...self, id: BUTTON_ME, name: 'Me' }], ...env.users]),
  }
}

const DATE_MODES = [
  { value: 'pick', label: 'A date' },
  { value: BUTTON_TODAY, label: 'Today' },
  { value: BUTTON_NOW, label: 'Now' },
]

/** One value: the cell editor on a stand-in row; a date can also be "today" or "now" at the press. */
function ValueField({ property, raw, onChange }: { property: Property; raw: unknown; onChange: (value: unknown) => void }) {
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null)
  const [editing, setEditing] = useState(false)
  if (property.type === 'title') {
    return <Input aria-label={property.name} value={typeof raw === 'string' ? raw : ''} maxLength={500} placeholder="Untitled" onChange={(event) => onChange(event.target.value)} />
  }
  const placeholder = raw === BUTTON_TODAY || raw === BUTTON_NOW ? raw : null
  const value = placeholder ? null : parseValue(property, raw)
  const row = standInRow(property, value)
  const inline = property.type === 'number' || property.type === 'url'
  const field = (
    <div className="relative min-w-0 flex-1">
      {editing && inline ? null : (
        <button
          ref={setAnchor}
          type="button"
          className="flex min-h-8 w-full min-w-0 items-center rounded-md border border-input px-2 py-1 text-left text-sm hover:bg-muted/60"
          onClick={() => (property.type === 'checkbox' ? onChange(value ? null : true) : setEditing(true))}
        >
          {value === null && property.type !== 'checkbox' ? <span className="text-muted-foreground/70">Empty (clears it)</span> : <CellView property={property} row={row} width={0} wrap />}
        </button>
      )}
      {editing ? (
        <CellEditor
          property={property}
          row={row}
          anchor={anchor}
          onCommit={(next: CellValue | null) => onChange(apiValue(next))}
          onTitle={() => {}}
          onClose={() => setEditing(false)}
        />
      ) : null}
    </div>
  )
  if (property.type !== 'date') return field
  return (
    <div className="flex min-w-0 items-center gap-2">
      <Select items={DATE_MODES} value={placeholder ?? 'pick'} onValueChange={(mode) => onChange(mode === 'pick' ? null : mode)}>
        <SelectTrigger aria-label={`${property.name} value`} className="w-28 shrink-0">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {DATE_MODES.map((mode) => (
            <SelectItem key={mode.value} value={mode.value}>
              {mode.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {placeholder ? null : field}
    </div>
  )
}

function standInRow(property: Property, value: CellValue | null): Row {
  const now = new Date(0).toISOString()
  return {
    id: 'button-value',
    title: '',
    icon: null,
    position: 0,
    version: 0,
    created_at: now,
    created_by: '',
    updated_at: now,
    updated_by: '',
    has_content: false,
    values: value ? { [property.id]: value } : {},
  }
}
