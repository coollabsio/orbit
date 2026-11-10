import { useRef, useState } from 'react'
import { Add, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Copy, EyeSlash, Filter, Settings, Trash } from 'reicon-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tip } from '@/components/common/Tip'
import { confirmAction } from '@/components/common/confirmAction'
import { isPaymentRequired } from '@orbit/api/problem'
import type { DatabasePropertyType, Property, ViewConfig } from '@orbit/database/types'
import { databaseErrorMessage } from '@orbit/database/errors'
import { previewPropertyUpdate, useCreateProperty, useDatabase, useDeleteProperty, useUpdateProperty } from './api'
import { useDatabaseEnv } from './context'
import { placeColumn } from './databaseLib'
import { filterable, isComputed, sortable } from '@orbit/database/meta'
import { CREATABLE_TYPES, NEW_ONLY_TYPES, PROPERTY_TYPES } from './propertyMeta'
import { RelationMenu, RelationSetup, RollupMenu, RollupSetup } from './RelationConfig'
import { ButtonConfigDialog } from './ButtonConfigDialog'
import { FormulaDialog } from './FormulaDialog'
import { UniqueIdPrefix } from './UniqueIdPrefix'

const CURRENCIES = ['USD', 'EUR', 'GBP', 'JPY', 'CHF', 'CAD', 'AUD', 'HUF']

interface ColumnHeaderMenuProps {
  property: Property
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
  onFilter: (propertyId: string) => void
}

/** The header cell menu (spec 7.2): name, type and format, sort, filter, hide, insert, duplicate, delete. */
export function ColumnHeaderMenu({ property, config, onConfig, onFilter }: ColumnHeaderMenuProps) {
  const { workspaceId, databaseId, canEditSchema } = useDatabaseEnv()
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const deleteProperty = useDeleteProperty(workspaceId, databaseId)
  const [name, setName] = useState(property.name)
  const [open, setOpen] = useState(false)
  /** "Insert left/right" asks for a name and a type in a popover at the header. */
  const [inserting, setInserting] = useState<'left' | 'right' | null>(null)
  const insertingRef = useRef(false)
  const [header, setHeader] = useState<HTMLButtonElement | null>(null)
  const isTitle = property.type === 'title'
  const [configuringButton, setConfiguringButton] = useState(false)
  const [editingFormula, setEditingFormula] = useState(false)
  /** A property inserted from this header; a formula or a button opens its editor. */
  const [created, setCreated] = useState<string | null>(null)

  const rename = () => {
    const next = name.trim()
    if (!canEditSchema || !next || next === property.name) return
    updateProperty.mutate({ propertyId: property.id, body: { name: next.slice(0, 200) } })
  }
  // The server converts the values: a dry run first tells whether any would be lost.
  const changeType = async (type: DatabasePropertyType) => {
    if (type === property.type) return
    let cleared: number
    try {
      cleared = (await previewPropertyUpdate(workspaceId, databaseId, property.id, { type })).cleared
    } catch (error) {
      if (!isPaymentRequired(error)) toast.error(databaseErrorMessage(error, 'Could not change the property.'))
      return
    }
    if (
      cleared > 0 &&
      !(await confirmAction({
        title: `Change “${property.name}” to ${PROPERTY_TYPES[type].label}?`,
        description: `${cleared.toLocaleString()} ${cleared === 1 ? 'value cannot' : 'values cannot'} be converted and will be cleared.`,
        confirmLabel: 'Change type',
        danger: true,
      }))
    ) {
      return
    }
    // A new formula opens its editor (it shows once the refetch has the formula).
    updateProperty.mutate({ propertyId: property.id, body: { type } }, { onSuccess: () => setEditingFormula(type === 'formula') })
  }
  const setConfig = (change: Record<string, unknown>) =>
    updateProperty.mutate({ propertyId: property.id, body: { config: { ...property.config, ...change } } })
  const sortBy = (direction: 'asc' | 'desc') => onConfig({ ...config, sorts: [{ property_id: property.id, direction }] })
  const startInsert = (side: 'left' | 'right') => {
    insertingRef.current = true
    setInserting(side)
  }
  const insert = (side: 'left' | 'right', name: string, type: DatabasePropertyType, propertyConfig?: NewPropertyConfig) => {
    setInserting(null)
    // The server adds the column to the end of every view; this view gets it next to the clicked column.
    createProperty.mutate(
      { name, type, ...(propertyConfig ? { config: propertyConfig } : {}) },
      {
        onSuccess: (created) => {
          onConfig(placeColumn(config, created.id, property.id, side))
          setCreated(created.id)
        },
      },
    )
  }
  const remove = async () => {
    if (
      await confirmAction({
        title: `Delete “${property.name}”?`,
        description: 'Its values in every row are deleted too, and every view loses its column.',
        confirmLabel: 'Delete property',
        danger: true,
      })
    ) {
      deleteProperty.mutate(property.id)
    }
  }

  const TypeIcon = PROPERTY_TYPES[property.type].icon
  return (
    <>
      <DropdownMenu
        open={open}
        onOpenChange={(next) => {
          setOpen(next)
          if (next) {
            setName(property.name)
            insertingRef.current = false
          } else rename()
        }}
        modal={false}
      >
        <DropdownMenuTrigger
          render={
            <button
              ref={setHeader}
              type="button"
              className="flex min-w-0 flex-1 items-center gap-1.5 px-2 text-left text-sm font-normal text-muted-foreground outline-none hover:bg-muted/60 focus-visible:bg-muted/60 aria-expanded:bg-muted/60"
            />
          }
        >
          <TypeIcon className="size-4 shrink-0" aria-hidden="true" />
          <span className="truncate">{property.name}</span>
        </DropdownMenuTrigger>
        {/* The insert popover takes the focus, not the header. */}
        <DropdownMenuContent align="start" className="w-60" finalFocus={() => !insertingRef.current}>
          <div className="p-1" onKeyDown={(event) => event.stopPropagation()}>
            <Input
              aria-label="Property name"
              value={name}
              readOnly={!canEditSchema}
              maxLength={200}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  rename()
                  setOpen(false)
                }
              }}
            />
          </div>
          {canEditSchema && !isTitle ? (
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>
                <Settings />
                Edit property
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-56">
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Type</DropdownMenuLabel>
                  <DropdownMenuRadioGroup value={property.type} onValueChange={(type) => void changeType(type as DatabasePropertyType)}>
                    {CREATABLE_TYPES.map((type) => {
                      const Icon = PROPERTY_TYPES[type].icon
                      return (
                        <DropdownMenuRadioItem key={type} value={type}>
                          <Icon className="text-muted-foreground" />
                          {PROPERTY_TYPES[type].label}
                        </DropdownMenuRadioItem>
                      )
                    })}
                  </DropdownMenuRadioGroup>
                </DropdownMenuGroup>
                {property.type === 'number' ? (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuGroup>
                      <DropdownMenuLabel>Format</DropdownMenuLabel>
                      <DropdownMenuRadioGroup
                        value={property.config.format === 'currency' ? `currency:${property.config.currency ?? 'USD'}` : property.config.format}
                        onValueChange={(value: string) =>
                          value.startsWith('currency:') ? setConfig({ format: 'currency', currency: value.slice(9) }) : setConfig({ format: value })
                        }
                      >
                        <DropdownMenuRadioItem value="number">Number</DropdownMenuRadioItem>
                        <DropdownMenuRadioItem value="number_with_commas">Number with commas</DropdownMenuRadioItem>
                        <DropdownMenuRadioItem value="percent">Percent</DropdownMenuRadioItem>
                        {CURRENCIES.map((currency) => (
                          <DropdownMenuRadioItem key={currency} value={`currency:${currency}`}>
                            {currency}
                          </DropdownMenuRadioItem>
                        ))}
                      </DropdownMenuRadioGroup>
                    </DropdownMenuGroup>
                  </>
                ) : null}
                {property.type === 'date' ? (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuGroup>
                      <DropdownMenuLabel>Date format</DropdownMenuLabel>
                      <DropdownMenuRadioGroup value={property.config.date_format} onValueChange={(value: string) => setConfig({ date_format: value })}>
                        <DropdownMenuRadioItem value="relative">Relative</DropdownMenuRadioItem>
                        <DropdownMenuRadioItem value="short">Short (Nov 9, 2026)</DropdownMenuRadioItem>
                        <DropdownMenuRadioItem value="iso">ISO (2026-11-09)</DropdownMenuRadioItem>
                      </DropdownMenuRadioGroup>
                    </DropdownMenuGroup>
                    <DropdownMenuGroup>
                      <DropdownMenuLabel>Time format</DropdownMenuLabel>
                      <DropdownMenuRadioGroup value={property.config.time_format} onValueChange={(value: string) => setConfig({ time_format: value })}>
                        <DropdownMenuRadioItem value="12h">12 hour</DropdownMenuRadioItem>
                        <DropdownMenuRadioItem value="24h">24 hour</DropdownMenuRadioItem>
                      </DropdownMenuRadioGroup>
                    </DropdownMenuGroup>
                  </>
                ) : null}
                {property.type === 'button' ? (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={() => setConfiguringButton(true)}>Edit button…</DropdownMenuItem>
                  </>
                ) : null}
                {property.type === 'formula' ? (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={() => setEditingFormula(true)}>Edit formula…</DropdownMenuItem>
                  </>
                ) : null}
                {property.type === 'unique_id' ? (
                  <>
                    <DropdownMenuSeparator />
                    <UniqueIdPrefix prefix={property.config.prefix} onSave={(prefix) => setConfig({ prefix })} />
                  </>
                ) : null}
                {property.type === 'relation' ? <RelationMenu property={property} /> : null}
                {property.type === 'rollup' ? <RollupMenu property={property} /> : null}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          ) : null}
          <DropdownMenuSeparator />
          {!filterable(property.type) ? null : (
            <>
              {sortable(property.type) ? (
                <>
                  <DropdownMenuItem onClick={() => sortBy('asc')}>
                    <ArrowUp />
                    Sort ascending
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => sortBy('desc')}>
                    <ArrowDown />
                    Sort descending
                  </DropdownMenuItem>
                </>
              ) : null}
              <DropdownMenuItem onClick={() => onFilter(property.id)}>
                <Filter />
                Filter
              </DropdownMenuItem>
            </>
          )}
          {isTitle ? null : (
            <DropdownMenuItem
              onClick={() =>
                onConfig({ ...config, columns: config.columns.map((column) => (column.property_id === property.id ? { ...column, hidden: true } : column)) })
              }
            >
              <EyeSlash />
              Hide
            </DropdownMenuItem>
          )}
          {canEditSchema ? (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={() => startInsert('left')}>
                <ArrowLeft />
                Insert left
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => startInsert('right')}>
                <ArrowRight />
                Insert right
              </DropdownMenuItem>
              {isTitle ? null : (
                <>
                  {property.type === 'unique_id' ? null : (
                    <DropdownMenuItem onClick={() => createProperty.mutate({ duplicate_of: property.id })}>
                      <Copy />
                      Duplicate
                    </DropdownMenuItem>
                  )}
                  <DropdownMenuItem variant="destructive" onClick={() => void remove()}>
                    <Trash />
                    Delete property
                  </DropdownMenuItem>
                </>
              )}
            </>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>
      <Popover
        open={inserting !== null}
        onOpenChange={(next) => {
          if (!next) setInserting(null)
        }}
      >
        {inserting ? (
          <PopoverContent anchor={header} align="start" className="w-60 gap-0 p-0">
            <NewPropertyForm relations onCreate={(name, type, propertyConfig) => insert(inserting, name, type, propertyConfig)} />
          </PopoverContent>
        ) : null}
      </Popover>
      {configuringButton && property.type === 'button' ? <ButtonConfigDialog property={property} onClose={() => setConfiguringButton(false)} /> : null}
      {editingFormula && property.type === 'formula' ? <FormulaDialog property={property} onClose={() => setEditingFormula(false)} /> : null}
      {created ? <NewPropertySetup propertyId={created} onClose={() => setCreated(null)} /> : null}
    </>
  )
}

/** A new formula or button opens its editor at once: without an expression or actions it does nothing. */
function NewPropertySetup({ propertyId, onClose }: { propertyId: string; onClose: () => void }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const property = useDatabase(workspaceId, databaseId).data?.properties.find((candidate) => candidate.id === propertyId)
  if (property?.type === 'formula') return <FormulaDialog property={property} onClose={onClose} />
  if (property?.type === 'button') return <ButtonConfigDialog property={property} onClose={onClose} />
  return null
}

/** The "+" header cell: a name and a type make a new property at the end of the views. */
export function AddPropertyButton() {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const [open, setOpen] = useState(false)
  const [created, setCreated] = useState<string | null>(null)
  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <Tip label="Add a property">
          <PopoverTrigger render={<Button type="button" variant="ghost" size="icon-sm" className="m-0.5 text-muted-foreground" aria-label="Add a property" />}>
            <Add />
          </PopoverTrigger>
        </Tip>
        <PopoverContent align="end" className="w-60 gap-0 p-0">
          <NewPropertyForm
            relations
            onCreate={(name, type, config) => {
              setOpen(false)
              // The server adds the column to the end of every view.
              createProperty.mutate({ name, type, ...(config ? { config } : {}) }, { onSuccess: (property) => setCreated(property.id) })
            }}
          />
        </PopoverContent>
      </Popover>
      {created ? <NewPropertySetup propertyId={created} onClose={() => setCreated(null)} /> : null}
    </>
  )
}

/** The config a new property is created with: only relations and rollups need one up front. */
export type NewPropertyConfig = Record<string, unknown>

/**
 * A new property's name field and type list: picking a type creates it (an empty name takes the type's label).
 * `relations`: also offers relation and rollup, which ask for their config first.
 */
export function NewPropertyForm({ onCreate, relations = false }: { onCreate: (name: string, type: DatabasePropertyType, config?: NewPropertyConfig) => void; relations?: boolean }) {
  const [name, setName] = useState('')
  const [setup, setSetup] = useState<'relation' | 'rollup' | null>(null)
  const nameFor = (type: DatabasePropertyType) => (name.trim() || PROPERTY_TYPES[type].label).slice(0, 200)
  if (setup === 'relation') return <RelationSetup onBack={() => setSetup(null)} onCreate={(config) => onCreate(nameFor('relation'), 'relation', config)} />
  if (setup === 'rollup') return <RollupSetup onBack={() => setSetup(null)} onCreate={(config) => onCreate(nameFor('rollup'), 'rollup', config)} />
  const types = relations ? [...CREATABLE_TYPES, ...NEW_ONLY_TYPES] : CREATABLE_TYPES
  return (
    <Command label="Property type" loop>
      <div className="p-1">
        <Input autoFocus aria-label="Property name" placeholder="Property name" value={name} maxLength={200} onChange={(event) => setName(event.target.value)} />
      </div>
      <CommandInput aria-label="Search types" placeholder="Type…" />
      <CommandList>
        <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">No types</CommandEmpty>
        <CommandGroup>
          {types.map((type) => {
            const Icon = PROPERTY_TYPES[type].icon
            return (
              <CommandItem
                key={type}
                value={type}
                keywords={[PROPERTY_TYPES[type].label]}
                onSelect={() => (type === 'relation' || type === 'rollup' ? setSetup(type) : onCreate(nameFor(type), type))}
              >
                <Icon className="text-muted-foreground" />
                {PROPERTY_TYPES[type].label}
                {isComputed(type) ? <span className="ml-auto text-xs text-muted-foreground">Automatic</span> : null}
              </CommandItem>
            )
          })}
        </CommandGroup>
      </CommandList>
    </Command>
  )
}
