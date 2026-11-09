import { useRef, useState } from 'react'
import { Add, ArrowSwapHorizontal, Eye, Filter, Kanban, Link, Lock, MoreH, Search, Sort, Unlock, Xmark } from 'reicon-react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Tip } from '@/components/common/Tip'
import type { DatabaseView, Property, ViewConfig } from '@orbit/database/types'
import { groupable } from '@orbit/database/view'
import { useSetPageLock } from '@/features/docs/api/pageOptions'
import { useDatabaseEnv } from './context'
import { PropertyPicker } from './FilterBar'
import { filterRules } from '@orbit/database/filter'
import { GripIcon } from './GripIcon'
import { PROPERTY_TYPES } from './propertyMeta'
import { SortPopover } from './SortPopover'

interface ViewToolbarProps {
  properties: Property[]
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
  /** Adds a rule on the property and opens its editor. */
  onAddFilter: (property: Property) => void
  /** Opens the advanced filter. */
  onAdvancedFilter: () => void
  search: string
  onSearch: (search: string) => void
  locked: boolean
  viewId: string
  viewType: DatabaseView['type']
  onNew: () => void
}

/** Filter, Sort, search, "⋯" (properties or a board's group and card properties, wrap cells, lock, link) and New (spec 7.1). */
export function ViewToolbar({ properties, config, onConfig, onAddFilter, onAdvancedFilter, search, onSearch, locked, viewId, viewType, onNew }: ViewToolbarProps) {
  const { workspaceId, databaseId, canEditValues } = useDatabaseEnv()
  const setLock = useSetPageLock(workspaceId)
  const [searching, setSearching] = useState(search !== '')
  const [propertiesOpen, setPropertiesOpen] = useState(false)
  const moreRef = useRef<HTMLButtonElement>(null)
  const rules = filterRules(config.filter).length
  const sorts = config.sorts.length

  const copyLink = () => {
    const url = new URL(`/docs/${databaseId}`, window.location.origin)
    url.searchParams.set('view', viewId)
    void navigator.clipboard.writeText(url.toString()).then(
      () => toast.success('Link to the view copied.'),
      () => toast.error('Could not copy the link.'),
    )
  }
  const toggleLock = () =>
    setLock.mutate(
      { pageId: databaseId, locked: !locked },
      {
        onSuccess: () => toast.success(locked ? 'Database unlocked.' : 'Database locked. Properties and views cannot change until it is unlocked.'),
        onError: () => toast.error(locked ? 'Could not unlock the database.' : 'Could not lock the database.'),
      },
    )

  return (
    <div className="flex shrink-0 items-center gap-0.5">
      <PropertyPicker
        properties={properties}
        label="Filter by"
        onPick={onAddFilter}
        onAdvanced={onAdvancedFilter}
        trigger={
          <Button type="button" variant="ghost" size="sm" data-active={rules > 0 || undefined} className="text-muted-foreground data-active:text-primary" aria-label={rules > 0 ? `Filter (${rules})` : 'Filter'}>
            <Filter />
            <span className="@max-lg/controls:hidden">Filter</span>
            {rules > 0 ? <Badge className="h-4 min-w-4 px-1 text-[10px] tabular-nums">{rules}</Badge> : null}
          </Button>
        }
      />
      <SortPopover
        properties={properties}
        config={config}
        onConfig={onConfig}
        trigger={
          <Button type="button" variant="ghost" size="sm" data-active={sorts > 0 || undefined} className="text-muted-foreground data-active:text-primary" aria-label={sorts > 0 ? `Sort (${sorts})` : 'Sort'}>
            <Sort />
            <span className="@max-lg/controls:hidden">Sort</span>
            {sorts > 0 ? <Badge className="h-4 min-w-4 px-1 text-[10px] tabular-nums">{sorts}</Badge> : null}
          </Button>
        }
      />
      {searching ? (
        <div className="flex h-7 w-48 items-center gap-1 rounded-md @max-lg/controls:w-24 bg-secondary/40 px-2 focus-within:ring-2 focus-within:ring-ring/50">
          <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
          <input
            autoFocus
            aria-label="Search this view"
            placeholder="Search…"
            className="min-w-0 flex-1 bg-transparent text-sm outline-none"
            value={search}
            onChange={(event) => onSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                onSearch('')
                setSearching(false)
              }
            }}
            onBlur={() => {
              if (search === '') setSearching(false)
            }}
          />
          {search ? (
            <button type="button" aria-label="Clear search" className="text-muted-foreground hover:text-foreground" onClick={() => onSearch('')}>
              <Xmark className="size-3" />
            </button>
          ) : null}
        </div>
      ) : (
        <Tip label="Search">
          <Button type="button" variant="ghost" size="icon-sm" aria-label="Search" className="text-muted-foreground" onClick={() => setSearching(true)}>
            <Search />
          </Button>
        </Tip>
      )}
      <DropdownMenu>
        <Tip label="View options">
          <DropdownMenuTrigger render={<Button ref={moreRef} type="button" variant="ghost" size="icon-sm" aria-label="View options" className="text-muted-foreground" />}>
            <MoreH />
          </DropdownMenuTrigger>
        </Tip>
        <DropdownMenuContent align="end" className="w-52">
          {viewType === 'board' ? <GroupByMenu properties={properties} config={config} onConfig={onConfig} /> : null}
          <DropdownMenuItem onClick={() => setPropertiesOpen(true)}>
            <Eye />
            {viewType === 'board' ? 'Card properties' : 'Properties'}
          </DropdownMenuItem>
          {viewType === 'table' ? (
            <DropdownMenuCheckboxItem checked={config.wrap_cells} onCheckedChange={(checked) => onConfig({ ...config, wrap_cells: checked })}>
              <ArrowSwapHorizontal />
              Wrap cells
            </DropdownMenuCheckboxItem>
          ) : null}
          <DropdownMenuSeparator />
          {canEditValues ? (
            <DropdownMenuItem disabled={setLock.isPending} onClick={toggleLock}>
              {locked ? <Unlock /> : <Lock />}
              {locked ? 'Unlock database' : 'Lock database'}
            </DropdownMenuItem>
          ) : null}
          <DropdownMenuItem onClick={copyLink}>
            <Link />
            Copy link to view
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <Popover open={propertiesOpen} onOpenChange={setPropertiesOpen}>
        <PopoverContent anchor={moreRef} align="end" className="w-64 gap-0 p-1">
          {viewType === 'board' ? (
            <CardPropertiesList properties={properties} config={config} onConfig={onConfig} />
          ) : (
            <PropertiesList properties={properties} config={config} onConfig={onConfig} />
          )}
        </PopoverContent>
      </Popover>
      {canEditValues ? (
        <Button type="button" size="sm" className="ml-1" onClick={onNew}>
          <Add />
          New
        </Button>
      ) : null}
    </div>
  )
}

/** Show, hide and reorder the view's columns. The title always shows. */
function PropertiesList({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  const dragFrom = useRef<number | null>(null)
  const byId = new Map(properties.map((property) => [property.id, property]))
  return (
    <ul aria-label="Properties" className="flex max-h-80 flex-col overflow-y-auto">
      {config.columns.map((column, index) => {
        const property = byId.get(column.property_id)
        if (!property) return null
        const Icon = PROPERTY_TYPES[property.type].icon
        return (
          <li
            key={column.property_id}
            className="flex h-8 items-center gap-2 rounded-md px-1.5 text-sm hover:bg-muted/60"
            onDragOver={(event) => {
              if (dragFrom.current !== null) event.preventDefault()
            }}
            onDrop={(event) => {
              event.preventDefault()
              const from = dragFrom.current
              dragFrom.current = null
              if (from === null || from === index) return
              const columns = [...config.columns]
              const [moved] = columns.splice(from, 1)
              columns.splice(index, 0, moved)
              onConfig({ ...config, columns })
            }}
          >
            <span
              draggable
              role="button"
              tabIndex={-1}
              aria-label={`Drag ${property.name}`}
              className="flex h-6 w-3 cursor-grab items-center justify-center text-muted-foreground"
              onDragStart={(event) => {
                dragFrom.current = index
                event.dataTransfer.effectAllowed = 'move'
                event.dataTransfer.setData('text/plain', property.name)
              }}
              onDragEnd={() => {
                dragFrom.current = null
              }}
            >
              <GripIcon />
            </span>
            <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate">{property.name}</span>
            <Checkbox
              aria-label={`Show ${property.name}`}
              checked={!column.hidden}
              disabled={property.type === 'title'}
              onCheckedChange={(checked) =>
                onConfig({ ...config, columns: config.columns.map((other) => (other.property_id === column.property_id ? { ...other, hidden: !checked } : other)) })
              }
            />
          </li>
        )
      })}
    </ul>
  )
}

/** A board's group property: a select or status property (the columns are its options). */
function GroupByMenu({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  const choices = properties.filter(groupable)
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger disabled={choices.length === 0}>
        <Kanban />
        Group by
        <span className="ml-auto max-w-20 truncate text-xs text-muted-foreground">{choices.find((property) => property.id === config.group_by)?.name}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="w-48">
        <DropdownMenuRadioGroup
          value={config.group_by ?? ''}
          onValueChange={(id: string) => {
            if (id !== config.group_by) onConfig({ ...config, group_by: id, group_order: [], hidden_groups: [] })
          }}
        >
          {choices.map((property) => {
            const Icon = PROPERTY_TYPES[property.type].icon
            return (
              <DropdownMenuRadioItem key={property.id} value={property.id}>
                <Icon className="text-muted-foreground" />
                <span className="truncate">{property.name}</span>
              </DropdownMenuRadioItem>
            )
          })}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

/** Show or hide the properties of a board's cards, after the title (which always shows). */
function CardPropertiesList({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  const shown = new Set(config.card_properties)
  return (
    <ul aria-label="Card properties" className="flex max-h-80 flex-col overflow-y-auto">
      {properties
        .filter((property) => property.type !== 'title')
        .map((property) => {
          const Icon = PROPERTY_TYPES[property.type].icon
          return (
            <li key={property.id} className="flex h-8 items-center gap-2 rounded-md px-1.5 text-sm hover:bg-muted/60">
              <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate">{property.name}</span>
              <Checkbox
                aria-label={`Show ${property.name}`}
                checked={shown.has(property.id)}
                onCheckedChange={(checked) =>
                  onConfig({
                    ...config,
                    card_properties: checked ? [...config.card_properties, property.id] : config.card_properties.filter((id) => id !== property.id),
                  })
                }
              />
            </li>
          )
        })}
    </ul>
  )
}
