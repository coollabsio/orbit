import { useState, type ReactElement } from 'react'
import { Add, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Copy, EyeSlash, Filter, Settings, Trash } from 'reicon-react'
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
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tip } from '@/components/common/Tip'
import { confirmAction } from '@/components/common/confirmAction'
import type { DatabasePropertyType, Property, Row, ViewConfig } from '@orbit/database/types'
import { useCreateProperty, useDeleteProperty, useUpdateProperty } from './api'
import { useDatabaseEnv } from './context'
import { placeColumn } from './databaseLib'
import { CREATABLE_TYPES, PROPERTY_TYPES, isComputed } from './propertyMeta'

const CURRENCIES = ['USD', 'EUR', 'GBP', 'JPY', 'CHF', 'CAD', 'AUD', 'HUF']

interface ColumnHeaderMenuProps {
  property: Property
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
  onFilter: (propertyId: string) => void
  /** The database's rows: when one stores a value, a type change asks first, as it may clear values. */
  rows: readonly Row[]
  trigger: ReactElement
}

/** The header cell menu (spec 7.2): name, type and format, sort, filter, hide, insert, duplicate, delete. */
export function ColumnHeaderMenu({ property, config, onConfig, onFilter, rows, trigger }: ColumnHeaderMenuProps) {
  const { workspaceId, databaseId, canEditSchema } = useDatabaseEnv()
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const deleteProperty = useDeleteProperty(workspaceId, databaseId)
  const [name, setName] = useState(property.name)
  const [open, setOpen] = useState(false)
  const isTitle = property.type === 'title'

  const rename = () => {
    const next = name.trim()
    if (!canEditSchema || !next || next === property.name) return
    updateProperty.mutate({ propertyId: property.id, body: { name: next.slice(0, 200) } })
  }
  const changeType = async (type: DatabasePropertyType) => {
    if (type === property.type) return
    if (
      type !== 'text' &&
      rows.some((row) => property.id in row.values) &&
      !(await confirmAction({
        title: `Change “${property.name}” to ${PROPERTY_TYPES[type].label}?`,
        description: 'Some values cannot be converted and will be cleared.',
        confirmLabel: 'Change type',
        danger: true,
      }))
    ) {
      return
    }
    updateProperty.mutate({ propertyId: property.id, body: { type } })
  }
  const setConfig = (change: Record<string, unknown>) =>
    updateProperty.mutate({ propertyId: property.id, body: { config: { ...property.config, ...change } } })
  const sortBy = (direction: 'asc' | 'desc') => onConfig({ ...config, sorts: [{ property_id: property.id, direction }] })
  const insert = (side: 'left' | 'right', base?: Property) =>
    createProperty.mutate(
      base ? { name: `${base.name} (copy)`.slice(0, 200), type: base.type, config: base.config } : { name: 'Property', type: 'text' },
      { onSuccess: (created) => onConfig(placeColumn(config, created.id, property.id, side)) },
    )
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

  return (
    <DropdownMenu
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (next) setName(property.name)
        else rename()
      }}
      modal={false}
    >
      {trigger}
      <DropdownMenuContent align="start" className="w-60">
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
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        ) : null}
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={() => sortBy('asc')}>
          <ArrowUp />
          Sort ascending
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => sortBy('desc')}>
          <ArrowDown />
          Sort descending
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => onFilter(property.id)}>
          <Filter />
          Filter
        </DropdownMenuItem>
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
            <DropdownMenuItem onClick={() => insert('left')}>
              <ArrowLeft />
              Insert left
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => insert('right')}>
              <ArrowRight />
              Insert right
            </DropdownMenuItem>
            {isTitle ? null : (
              <>
                <DropdownMenuItem onClick={() => insert('right', property)}>
                  <Copy />
                  Duplicate
                </DropdownMenuItem>
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
  )
}

/** The "+" header cell: a name and a type make a new property at the end of the views. */
export function AddPropertyButton() {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const create = (type: DatabasePropertyType) => {
    setOpen(false)
    // The server adds the column to the end of every view.
    createProperty.mutate({ name: (name.trim() || PROPERTY_TYPES[type].label).slice(0, 200), type })
  }
  return (
    <Popover open={open} onOpenChange={(next) => { setOpen(next); if (next) setName('') }}>
      <Tip label="Add a property">
        <PopoverTrigger render={<Button type="button" variant="ghost" size="icon-sm" className="m-0.5 text-muted-foreground" aria-label="Add a property" />}>
          <Add />
        </PopoverTrigger>
      </Tip>
      <PopoverContent align="end" className="w-60 gap-0 p-0">
        <Command label="Property type" loop>
          <div className="p-1">
            <Input autoFocus aria-label="Property name" placeholder="Property name" value={name} maxLength={200} onChange={(event) => setName(event.target.value)} />
          </div>
          <CommandInput aria-label="Search types" placeholder="Type…" />
          <CommandList>
            <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">No types</CommandEmpty>
            <CommandGroup>
              {CREATABLE_TYPES.map((type) => {
                const Icon = PROPERTY_TYPES[type].icon
                return (
                  <CommandItem key={type} value={type} keywords={[PROPERTY_TYPES[type].label]} onSelect={() => create(type)}>
                    <Icon className="text-muted-foreground" />
                    {PROPERTY_TYPES[type].label}
                    {isComputed(type) ? <span className="ml-auto text-xs text-muted-foreground">Automatic</span> : null}
                  </CommandItem>
                )
              })}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
