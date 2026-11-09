import { useState } from 'react'
import { Add } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { apiValue, cellOf } from '@orbit/database/values'
import { repairConfig } from '@orbit/database/view'
import type { CellValue, Property, Row, SelectOption } from '@orbit/database/types'
import { useCreateProperty, useDatabase, useUpdateProperty, useUpdateValues } from './api'
import { CellEditor } from './cells/CellEditor'
import { CellView } from './cells/CellView'
import { NewPropertyForm } from './ColumnHeaderMenu'
import { DatabaseEnvContext, useDatabaseEnv, useDatabaseEnvValue } from './context'
import { isComputed } from '@orbit/database/meta'
import { PROPERTY_TYPES } from './propertyMeta'

interface PropertyListProps {
  workspaceId: string
  databaseId: string
  rowId: string
  /** The view whose column order the list follows; the database's first view when absent or gone. */
  viewId?: string
}

/**
 * The properties of a row page under its title (spec 7.5): every property but the title, hidden ones included, each
 * with the table's cell editor. The title stays the page title field.
 */
export function PropertyList({ workspaceId, databaseId, rowId, viewId }: PropertyListProps) {
  const query = useDatabase(workspaceId, databaseId)
  const env = useDatabaseEnvValue(workspaceId, databaseId, query.data?.database.locked ?? true)
  const database = query.data
  const row = database?.rows.find((candidate) => candidate.id === rowId)
  if (!database || !row) return null

  const views = [...database.views].sort((a, b) => a.position - b.position)
  const view = views.find((candidate) => candidate.id === viewId) ?? views[0]
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const order = view ? repairConfig(view, database.properties).columns.map((column) => column.property_id) : database.properties.map((property) => property.id)
  const properties = order.flatMap((id) => {
    const property = byId.get(id)
    return property && property.type !== 'title' ? [property] : []
  })

  return (
    <DatabaseEnvContext.Provider value={env}>
      <div className="mb-5 flex flex-col border-b border-border pb-4" data-print-hide="" aria-label="Properties" role="group">
        {properties.map((property) => (
          <PropertyLine key={property.id} property={property} row={row} />
        ))}
        {env.canEditSchema ? <AddProperty /> : null}
      </div>
    </DatabaseEnvContext.Provider>
  )
}

function PropertyLine({ property, row }: { property: Property; row: Row }) {
  const { workspaceId, databaseId, canEditValues, canEditSchema } = useDatabaseEnv()
  const updateValues = useUpdateValues(workspaceId, databaseId)
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const [anchor, setAnchor] = useState<HTMLDivElement | null>(null)
  const [editing, setEditing] = useState(false)
  const Icon = PROPERTY_TYPES[property.type].icon
  const editable = canEditValues && !isComputed(property.type)
  const value = cellOf(row, property)
  const commit = (next: CellValue | null) => updateValues.mutate({ rowId: row.id, values: { [property.id]: apiValue(next) } })
  const saveOptions = (options: SelectOption[]) =>
    updateProperty.mutateAsync({ propertyId: property.id, body: { config: { ...property.config, options } } }).then(() => undefined)
  const checkbox = property.type === 'checkbox'
  const opensEditor = editable && !checkbox
  const inline = property.type === 'number' || property.type === 'url'

  return (
    <div className="flex min-h-[34px] items-start gap-1 text-sm">
      <div className="flex h-[34px] w-40 shrink-0 items-center gap-1.5 px-1.5 text-muted-foreground">
        <Icon className="size-4 shrink-0" aria-hidden="true" />
        <span className="truncate">{property.name}</span>
      </div>
      <div
        ref={setAnchor}
        role={opensEditor ? 'button' : undefined}
        tabIndex={opensEditor ? 0 : undefined}
        aria-label={opensEditor ? `Edit ${property.name}` : undefined}
        data-editable={opensEditor || undefined}
        className="flex min-h-[34px] min-w-0 flex-1 flex-wrap items-center rounded-sm px-2 py-1 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 data-editable:cursor-pointer data-editable:hover:bg-muted/60"
        onClick={(event) => {
          if (!opensEditor || editing || (event.target as HTMLElement).closest('a')) return
          setEditing(true)
        }}
        onKeyDown={(event) => {
          if (!opensEditor || editing || event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' ')) return
          event.preventDefault()
          setEditing(true)
        }}
      >
        {editing && inline ? null : checkbox ? (
          <span className="flex w-4">
            <CellView property={property} row={row} width={16} wrap={false} onToggle={editable ? () => commit(value ? null : { kind: 'checked' }) : undefined} />
          </span>
        ) : value === null && !isComputed(property.type) ? (
          <span className="text-muted-foreground/70">Empty</span>
        ) : (
          // Wrapped cells show every item, so the width is not measured.
          <CellView property={property} row={row} width={0} wrap />
        )}
        {editing ? (
          <CellEditor
            property={property}
            row={row}
            anchor={anchor}
            onCommit={commit}
            onTitle={() => {}}
            onOptions={canEditSchema ? saveOptions : undefined}
            onClose={() => setEditing(false)}
          />
        ) : null}
      </div>
    </div>
  )
}

/** "+ Add a property" under the list: the name and type form of the table's "+" column. */
function AddProperty() {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger render={<Button type="button" variant="ghost" size="sm" className="mt-0.5 w-fit gap-1.5 px-1.5 font-normal text-muted-foreground" />}>
        <Add className="size-4" />
        Add a property
      </PopoverTrigger>
      <PopoverContent align="start" className="w-60 gap-0 p-0">
        <NewPropertyForm
          onCreate={(name, type) => {
            setOpen(false)
            createProperty.mutate({ name, type })
          }}
        />
      </PopoverContent>
    </Popover>
  )
}
