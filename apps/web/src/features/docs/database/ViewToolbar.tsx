import { useRef, useState, type ReactNode } from 'react'
import { ArrowSwapHorizontal, Calendar, DocumentDownload, DocumentUpload, Eye, EyeSlash, Filter, Kanban, Link, Lock, MoreH, RowHorizontal, Search, Sort, Unlock, Xmark } from 'reicon-react'
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
import { filterable, sortable } from '@orbit/database/meta'
import { groupable, laneable, showsCards, withGroupBy } from '@orbit/database/view'
import { datable, weekdayName, type Weekday } from '@orbit/database/calendar'
import { addDays } from '@orbit/database/dates'
import type { Template } from '@orbit/database/templates'
import { useSetPageLock } from '@/features/docs/api/pageOptions'
import { useDatabaseEnv } from './context'
import { setWeekStart, useWeekStart } from './weekStart'
import { MergeCsvDialog } from './CsvImport'
import { GalleryMenus } from './DatabaseGallery'
import { TimelineMenus } from './DatabaseTimeline'
import { importSummary } from './csvImportText'
import { PropertyPicker } from './FilterBar'
import { filterRules } from '@orbit/database/filter'
import { GripIcon } from './GripIcon'
import { NewRowButton } from './NewRowButton'
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
  viewType: DatabaseView['type']
  /** A new row from the template (`null`: empty), opened in the peek. */
  onNew: (templateId: string | null) => void
  templates: readonly Template[]
  onEditTemplate: (templateId: string) => void
  /** Downloads the rows the view shows as CSV. */
  onExportCsv: () => void
  /** The path "Copy link to view" copies. */
  viewLink: string
}

/** Filter, Sort, search, "⋯" (properties or a board's group and card properties, wrap cells, lock, link) and New (spec 7.1). */
export function ViewToolbar({ properties, config, onConfig, onAddFilter, onAdvancedFilter, search, onSearch, locked, viewType, onNew, onExportCsv, templates, onEditTemplate, viewLink }: ViewToolbarProps) {
  const { workspaceId, databaseId, canEditValues, canEditSchema } = useDatabaseEnv()
  const [merging, setMerging] = useState(false)
  const setLock = useSetPageLock(workspaceId)
  const [searching, setSearching] = useState(search !== '')
  const [propertiesOpen, setPropertiesOpen] = useState(false)
  const moreRef = useRef<HTMLButtonElement>(null)
  const rules = filterRules(config.filter).length
  const sorts = config.sorts.length

  const copyLink = () => {
    void navigator.clipboard.writeText(new URL(viewLink, window.location.origin).toString()).then(
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
    <div className="ml-auto flex min-w-0 items-center gap-0.5">
      <PropertyPicker
        properties={properties.filter((property) => filterable(property.type))}
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
        properties={properties.filter((property) => sortable(property.type))}
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
        <div className="flex h-7 w-48 min-w-16 items-center gap-1 rounded-md @max-lg/controls:w-24 bg-secondary/40 px-2 focus-within:ring-2 focus-within:ring-ring/50">
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
          {viewType === 'board' ? <GroupMenus properties={properties} config={config} onConfig={onConfig} /> : null}
          {viewType === 'calendar' ? <CalendarMenus properties={properties} config={config} onConfig={onConfig} /> : null}
          {viewType === 'gallery' ? <GalleryMenus properties={properties} config={config} onConfig={onConfig} /> : null}
          {viewType === 'timeline' ? (
            <TimelineMenus properties={properties} config={config} onConfig={onConfig} />
          ) : (
            <DropdownMenuItem onClick={() => setPropertiesOpen(true)}>
              <Eye />
              {showsCards(viewType) ? 'Card properties' : 'Properties'}
            </DropdownMenuItem>
          )}
          {viewType === 'board' ? (
            <DropdownMenuCheckboxItem checked={config.hide_empty_groups} onCheckedChange={(checked) => onConfig({ ...config, hide_empty_groups: checked })}>
              <EyeSlash />
              Hide empty groups
            </DropdownMenuCheckboxItem>
          ) : null}
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
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={onExportCsv}>
            <DocumentDownload />
            Export view as CSV
          </DropdownMenuItem>
          {canEditValues ? (
            <DropdownMenuItem onClick={() => setMerging(true)}>
              <DocumentUpload />
              Merge with CSV
            </DropdownMenuItem>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>
      {merging ? (
        <MergeCsvDialog
          workspaceId={workspaceId}
          databaseId={databaseId}
          properties={properties}
          canAddProperties={canEditSchema}
          onClose={() => setMerging(false)}
          onMerged={(result) => {
            setMerging(false)
            toast.success(importSummary(result))
          }}
        />
      ) : null}
      <Popover open={propertiesOpen} onOpenChange={setPropertiesOpen}>
        <PopoverContent anchor={moreRef} align="end" className="w-64 gap-0 p-1">
          {showsCards(viewType) ? (
            <CardPropertiesList properties={properties} config={config} onConfig={onConfig} />
          ) : (
            <PropertiesList properties={properties} config={config} onConfig={onConfig} />
          )}
        </PopoverContent>
      </Popover>
      {canEditValues ? (
        <NewRowButton templates={templates} config={config} onConfig={onConfig} onNew={onNew} onEditTemplate={onEditTemplate} />
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

const NO_LANES = { sub_group_by: null, sub_group_order: [], hidden_sub_groups: [] } satisfies Partial<ViewConfig>

/**
 * A board's "Group by" (a select, status, multi-select or person property: its groups are the columns) and
 * "Sub-group" (one of those or a checkbox, not the group property: its groups are the lanes; or None).
 */
function GroupMenus({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  return (
    <>
      <GroupMenu
        label="Group by"
        icon={<Kanban />}
        choices={properties.filter(groupable)}
        value={config.group_by}
        onChange={(id) => onConfig(withGroupBy(config, id))}
      />
      <GroupMenu
        label="Sub-group"
        icon={<RowHorizontal />}
        choices={properties.filter((property) => laneable(property) && property.id !== config.group_by)}
        value={config.sub_group_by}
        onChange={(id) => onConfig({ ...config, ...NO_LANES, sub_group_by: id })}
        none
      />
    </>
  )
}

/** The days a week starts on somewhere: Sunday, Monday and Saturday. */
const WEEK_STARTS: readonly Weekday[] = [0, 1, 6]
/** A Sunday: the day of weekday `n` is `n` days later. */
const SUNDAY = '2026-11-01'

/** A calendar's "Calendar by" (its date property) and "Start week on" (this browser only, for every calendar). */
function CalendarMenus({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  const { ctx } = useDatabaseEnv()
  const weekStart = useWeekStart(ctx.locale)
  return (
    <>
      <GroupMenu
        label="Calendar by"
        icon={<Calendar />}
        choices={properties.filter(datable)}
        value={config.date_property_id ?? null}
        onChange={(id) => onConfig({ ...config, ...(id ? { date_property_id: id } : {}) })}
      />
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          <Calendar />
          Start week on
          <span className="ml-auto text-xs text-muted-foreground">{weekdayName(addDays(SUNDAY, weekStart), ctx.locale)}</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent className="w-40">
          <DropdownMenuRadioGroup value={String(weekStart)} onValueChange={(day: string) => setWeekStart(Number(day) as Weekday)}>
            {WEEK_STARTS.map((day) => (
              <DropdownMenuRadioItem key={day} value={String(day)}>
                {weekdayName(addDays(SUNDAY, day), ctx.locale, 'long')}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
    </>
  )
}

function GroupMenu({ label, icon, choices, value, onChange, none = false }: { label: string; icon: ReactNode; choices: Property[]; value: string | null; onChange: (id: string | null) => void; none?: boolean }) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger disabled={choices.length === 0}>
        {icon}
        {label}
        <span className="ml-auto max-w-20 truncate text-xs text-muted-foreground">{choices.find((property) => property.id === value)?.name ?? (none ? 'None' : '')}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="w-48">
        <DropdownMenuRadioGroup
          value={value ?? ''}
          onValueChange={(id: string) => {
            const next = id === '' ? null : id
            if (next !== value) onChange(next)
          }}
        >
          {none ? <DropdownMenuRadioItem value="">None</DropdownMenuRadioItem> : null}
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

/** Show or hide the properties of a board's or a calendar's cards, after the title (which always shows). */
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
