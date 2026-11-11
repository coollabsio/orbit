import { useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Add, Calendar, CalendarDate, Eye, SidebarLeft } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import {
  DropdownMenuCheckboxItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@/components/ui/dropdown-menu'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { addDays, localDay, pointDay } from '@orbit/database/dates'
import type { Database, Property, Row, ViewConfig } from '@orbit/database/types'
import {
  ZOOMS,
  ZOOM_LABELS,
  axis as axisOf,
  barOf,
  dayIndex,
  dayValues,
  moveBar,
  spanValues,
  timelineOf,
  timelineRange,
  type AxisSpan,
  type Bar,
  type BarEdge,
  type TimelineZoom,
} from '@orbit/database/timeline'
import { useUpdateValues } from './api'
import { CellView } from './cells/CellView'
import { useDatabaseEnv } from './context'
import { PROPERTY_TYPES } from './propertyMeta'

const ROW_HEIGHT = 34
const HEADER_HEIGHT = 52
const TITLE_WIDTH = 240
const COLUMN_WIDTH = 140
/** The table pane takes at most this share of the timeline's width; columns that do not fit are left out. */
const PANE_SHARE = 2 / 3
/** A bar narrower than this shows its title after it. */
const LABEL_INSIDE_MIN = 72
const MIN_BAR_WIDTH = 6
const EDGE_WIDTH = 6
/** A bar narrower than this has no edge handles: the edges would cover it, so it could not move. */
const RESIZE_MIN = 3 * EDGE_WIDTH
/** Pointer travel (px) before a press on a bar becomes a drag. */
const DRAG_SLOP = 3

interface DatabaseTimelineProps {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched, sorted, with new rows pinned). */
  rows: Row[]
  onConfig: (config: ViewConfig) => void
  /** "New": a row opened in the peek. */
  onAddRow: () => void
  onOpenRow: (rowId: string) => void
  filtered: boolean
  onClearFilters: () => void
  /** The full page: the timeline fills the window's height; a block is shorter. */
  page: boolean
}

type Item = { kind: 'row'; row: Row; bar: Bar | null } | { kind: 'no-date'; count: number }

type Drag = { rowId: string; edge: BarEdge; x: number; days: number; moved: boolean }

/**
 * The timeline view (spec P3 16.2): a table pane (title and `table_columns`, can hide) left of a scrolling time axis
 * with a bar per row. Drag a bar to move both dates, an edge to change one. Rows without a date list under "No date";
 * a click on their track sets a one-day date. Rows are virtualized in the timeline's own scroller.
 */
export function DatabaseTimeline({ database, config, rows, onConfig, onAddRow, onOpenRow, filtered, onClearFilters, page }: DatabaseTimelineProps) {
  const { workspaceId, databaseId, canEditValues, ctx } = useDatabaseEnv()
  const updateValues = useUpdateValues(workspaceId, databaseId)
  const timeline = timelineOf(config, database.properties)
  const title = database.properties.find((property) => property.type === 'title')
  const today = localDay(ctx.now, ctx.timeZone)
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  const [scrollerWidth, setScrollerWidth] = useState<number | null>(null)
  useLayoutEffect(() => {
    if (!scroller) return
    setScrollerWidth(scroller.clientWidth)
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(([entry]) => setScrollerWidth(entry!.contentRect.width))
    observer.observe(scroller)
    return () => observer.disconnect()
  }, [scroller])
  const [drag, setDrag] = useState<Drag | null>(null)
  const [hover, setHover] = useState<{ rowId: string; day: string } | null>(null)

  const dated: Item[] = []
  const undated: Item[] = []
  for (const row of rows) {
    const bar = barOf(row, timeline, ctx.timeZone)
    ;(bar ? dated : undated).push({ kind: 'row', row, bar })
  }
  const items: Item[] = undated.length > 0 ? [...dated, { kind: 'no-date', count: undated.length }, ...undated] : dated
  const bars = items.flatMap((item) => (item.kind === 'row' && item.bar ? [item.bar] : []))
  const axis = axisOf(timeline.zoom, timelineRange(bars, today, timeline.zoom), ctx.locale)
  const axisWidth = axis.days * axis.dayWidth
  // A narrow timeline (a phone) keeps room for the axis: a pane wider than the scroller would hide every bar.
  const paneRoom = scrollerWidth === null ? Infinity : scrollerWidth * PANE_SHARE
  const showTable = timeline.showTable && paneRoom >= TITLE_WIDTH
  const columns = showTable ? timeline.columns.slice(0, Math.floor((paneRoom - TITLE_WIDTH) / COLUMN_WIDTH)) : []
  const tableWidth = showTable ? TITLE_WIDTH + columns.length * COLUMN_WIDTH : 0
  const xOf = (day: string) => dayIndex(axis.start, day) * axis.dayWidth

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
  })

  const scrollToToday = (behavior: ScrollBehavior = 'smooth') => {
    if (!scroller) return
    const visible = scroller.clientWidth - tableWidth
    scroller.scrollTo({ left: Math.max(0, xOf(today) - visible / 3), behavior })
  }
  // Opens at today, and again at each zoom and pane width.
  const scrolledFor = useRef<string | null>(null)
  useLayoutEffect(() => {
    const key = `${timeline.zoom}:${axis.start}:${tableWidth}`
    if (!scroller || scrolledFor.current === key) return
    scrolledFor.current = key
    scrollToToday('instant')
  })

  const editable = canEditValues && timeline.start !== null
  const startDrag = (event: ReactPointerEvent<HTMLElement>, rowId: string, edge: BarEdge) => {
    if (event.button !== 0) return
    event.stopPropagation()
    if (!editable) {
      if (edge === 'both') onOpenRow(rowId)
      return
    }
    event.currentTarget.setPointerCapture(event.pointerId)
    setDrag({ rowId, edge, x: event.clientX, days: 0, moved: false })
  }
  const moveDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (!drag) return
    const dx = event.clientX - drag.x
    const moved = drag.moved || Math.abs(dx) > DRAG_SLOP
    const days = Math.round(dx / axis.dayWidth)
    if (moved !== drag.moved || days !== drag.days) setDrag({ ...drag, moved, days })
  }
  const endDrag = (row: Row, bar: Bar) => {
    if (!drag) return
    setDrag(null)
    if (!drag.moved) {
      onOpenRow(row.id)
      return
    }
    if (drag.days === 0) return
    const values = spanValues(row, timeline, moveBar(bar, drag.days, drag.edge, ctx.timeZone), ctx.timeZone)
    if (Object.keys(values).length > 0) updateValues.mutate({ rowId: row.id, values })
  }

  return (
    <div className="flex flex-col gap-2 pb-4">
      <div className="flex items-center gap-2 text-sm">
        {timeline.start === null ? (
          <span className="text-muted-foreground">Add a date property to place rows on the timeline.</span>
        ) : rows.length === 0 && filtered && database.rows.length > 0 ? (
          <span className="flex items-center gap-2 text-muted-foreground">
            No results
            <Button type="button" variant="outline" size="sm" onClick={onClearFilters}>
              Clear filters
            </Button>
          </span>
        ) : null}
        <span className="flex-1" />
        <Select value={timeline.zoom} onValueChange={(zoom) => onConfig({ ...config, zoom: zoom as TimelineZoom })}>
          <SelectTrigger size="sm" aria-label="Zoom" className="w-28">
            <SelectValue>{ZOOM_LABELS[timeline.zoom]}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {ZOOMS.map((zoom) => (
              <SelectItem key={zoom} value={zoom}>
                {ZOOM_LABELS[zoom]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button type="button" variant="outline" size="sm" onClick={() => scrollToToday()}>
          Today
        </Button>
      </div>
      <div
        ref={setScroller}
        role="grid"
        aria-label="Timeline"
        aria-busy={updateValues.isPending}
        className="relative w-full overflow-auto rounded-md border border-border [contain:inline-size]"
        style={{ maxHeight: page ? 'max(320px, calc(100dvh - 260px))' : 480 }}
      >
        <div className="relative" style={{ width: tableWidth + axisWidth }}>
          <div role="row" className="sticky top-0 z-20 flex border-b border-border bg-background" style={{ height: HEADER_HEIGHT }}>
            {showTable ? <TableHeader title={title} columns={columns} width={tableWidth} /> : null}
            <div className="relative shrink-0" style={{ width: axisWidth }}>
              {axis.groups.map((group) => (
                <div key={group.day} className="absolute top-0 h-6 border-l border-border" style={{ left: group.start * axis.dayWidth, width: group.days * axis.dayWidth }} />
              ))}
              <GroupLabels scroller={scroller} groups={axis.groups} dayWidth={axis.dayWidth} />
              {axis.ticks.map((tick) => (
                <div
                  key={tick.day}
                  className={cn('absolute top-6 flex h-[26px] items-center justify-center border-l border-border/60 text-[0.6875rem] whitespace-nowrap text-muted-foreground', tick.day === today && 'font-semibold text-primary')}
                  style={{ left: tick.start * axis.dayWidth, width: tick.days * axis.dayWidth }}
                >
                  {tick.days * axis.dayWidth >= 18 ? tick.label : null}
                </div>
              ))}
            </div>
          </div>
          <div className="relative" style={{ height: virtualizer.getTotalSize() + (canEditValues ? ROW_HEIGHT : 0) }}>
            {/* Tick lines and the today line, under the rows. */}
            <div aria-hidden="true" className="pointer-events-none absolute inset-y-0" style={{ left: tableWidth, width: axisWidth }}>
              {axis.ticks.map((tick) => (
                <div key={tick.day} className="absolute inset-y-0 border-l border-border/40" style={{ left: tick.start * axis.dayWidth }} />
              ))}
              <div className="absolute inset-y-0 w-px bg-primary/70" style={{ left: xOf(today) + axis.dayWidth / 2 }} />
            </div>
            {virtualizer.getVirtualItems().map((virtual) => {
              const item = items[virtual.index]!
              if (item.kind === 'no-date') {
                return (
                  <div key="no-date" role="row" className="absolute left-0 flex w-full items-end border-b border-border bg-background" style={{ top: virtual.start, height: ROW_HEIGHT }}>
                    <span className="sticky left-0 px-2 pb-1.5 text-xs font-medium text-muted-foreground">No date ({item.count})</span>
                  </div>
                )
              }
              const { row } = item
              const dragging = drag?.rowId === row.id && drag.moved ? drag : null
              const bar = item.bar && dragging ? movedBar(item.bar, dragging, ctx.timeZone) : item.bar
              const ghost = !item.bar && hover?.rowId === row.id ? hover.day : null
              return (
                <div key={row.id} role="row" className="group/row absolute left-0 flex w-full border-b border-border/60 hover:bg-muted/30" style={{ top: virtual.start, height: ROW_HEIGHT }}>
                  {showTable ? <TableCells row={row} title={title} columns={columns} width={tableWidth} onOpen={() => onOpenRow(row.id)} /> : null}
                  <div
                    className={cn('relative shrink-0', !item.bar && editable && 'cursor-cell')}
                    style={{ width: axisWidth }}
                    onPointerMove={(event) => {
                      if (item.bar || !editable) return
                      const x = event.clientX - event.currentTarget.getBoundingClientRect().left
                      const day = addDays(axis.start, Math.floor(x / axis.dayWidth))
                      if (hover?.rowId !== row.id || hover.day !== day) setHover({ rowId: row.id, day })
                    }}
                    onPointerLeave={() => setHover(null)}
                    onClick={() => {
                      if (ghost && timeline.start) updateValues.mutate({ rowId: row.id, values: dayValues(timeline, ghost) })
                    }}
                  >
                    {!showTable && !item.bar ? (
                      <span className="pointer-events-none sticky left-0 flex h-full w-fit max-w-60 items-center truncate px-2 text-sm text-muted-foreground">{row.title || 'Untitled'}</span>
                    ) : null}
                    {ghost ? (
                      <div aria-hidden="true" className="absolute top-1.5 rounded-sm border border-dashed border-primary/60 bg-primary/10" style={{ left: xOf(ghost), width: Math.max(axis.dayWidth, MIN_BAR_WIDTH), height: ROW_HEIGHT - 12 }} />
                    ) : null}
                    {bar && item.bar ? (
                      <BarView
                        row={row}
                        left={xOf(bar.startDay)}
                        labelLeft={tableWidth}
                        width={Math.max((dayIndex(bar.startDay, bar.endDay) + 1) * axis.dayWidth, MIN_BAR_WIDTH)}
                        editable={editable}
                        dragging={dragging !== null}
                        onPointerDown={(event, edge) => startDrag(event, row.id, edge)}
                        onPointerMove={moveDrag}
                        onPointerUp={() => endDrag(row, item.bar!)}
                        onPointerCancel={() => setDrag(null)}
                      />
                    ) : null}
                  </div>
                </div>
              )
            })}
            {canEditValues ? (
              <div className="absolute left-0 flex w-full" style={{ top: virtualizer.getTotalSize(), height: ROW_HEIGHT }}>
                <button type="button" className="sticky left-0 flex items-center gap-1.5 px-2 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground" onClick={onAddRow}>
                  <Add className="size-4" aria-hidden="true" />
                  New
                </button>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  )
}

const GROUP_LABEL_INSET = 8

/**
 * The month (or year) labels over the axis. Each stays at the start of its group's visible part and shortens with it,
 * so a group mostly scrolled away (under the table pane or past the left edge) keeps its label inside instead of
 * pushing it out. Axis x equals the scroller's scrollLeft at the visible start, right of the sticky table pane.
 */
function GroupLabels({ scroller, groups, dayWidth }: { scroller: HTMLDivElement | null; groups: AxisSpan[]; dayWidth: number }) {
  const [scrollLeft, setScrollLeft] = useState(0)
  useLayoutEffect(() => {
    if (!scroller) return
    const update = () => setScrollLeft(scroller.scrollLeft)
    update()
    scroller.addEventListener('scroll', update, { passive: true })
    return () => scroller.removeEventListener('scroll', update)
  }, [scroller])
  return groups.map((group) => {
    const end = (group.start + group.days) * dayWidth
    const left = Math.max(group.start * dayWidth, scrollLeft) + GROUP_LABEL_INSET
    const maxWidth = end - left - GROUP_LABEL_INSET
    if (maxWidth <= 0) return null
    return (
      <span key={group.day} className="absolute top-0 truncate text-xs leading-6 font-medium whitespace-nowrap" style={{ left, maxWidth }}>
        {group.label}
      </span>
    )
  })
}

/** The bar as it shows during a drag. */
function movedBar(bar: Bar, drag: Drag, timeZone: string): Bar {
  const span = moveBar(bar, drag.days, drag.edge, timeZone)
  return { span, startDay: pointDay(span.start, timeZone), endDay: pointDay(span.end, timeZone) }
}

function TableHeader({ title, columns, width }: { title: Property | undefined; columns: Property[]; width: number }) {
  const cell = 'flex items-end gap-1.5 border-r border-border/60 px-2 pb-1.5 text-xs text-muted-foreground'
  return (
    <div className="sticky left-0 z-10 flex shrink-0 border-r border-border bg-background" style={{ width }}>
      <div className={cell} style={{ width: TITLE_WIDTH }}>
        {title?.name ?? 'Name'}
      </div>
      {columns.map((property) => {
        const Icon = PROPERTY_TYPES[property.type].icon
        return (
          <div key={property.id} className={cn(cell, 'min-w-0')} style={{ width: COLUMN_WIDTH }}>
            <Icon className="size-3.5 shrink-0" aria-hidden="true" />
            <span className="truncate">{property.name}</span>
          </div>
        )
      })}
    </div>
  )
}

function TableCells({ row, title, columns, width, onOpen }: { row: Row; title: Property | undefined; columns: Property[]; width: number; onOpen: () => void }) {
  return (
    <div className="sticky left-0 z-10 flex shrink-0 border-r border-border bg-background group-hover/row:bg-muted" style={{ width }}>
      <button type="button" role="gridcell" className="flex min-w-0 items-center px-2 text-left text-sm outline-none hover:underline focus-visible:bg-muted" style={{ width: TITLE_WIDTH }} onClick={onOpen}>
        {title ? <CellView property={title} row={row} width={TITLE_WIDTH - 16} wrap={false} /> : null}
      </button>
      {columns.map((property) => (
        <div key={property.id} role="gridcell" className="flex min-w-0 items-center px-2 text-[0.8125rem]" style={{ width: COLUMN_WIDTH }}>
          <CellView property={property} row={row} width={COLUMN_WIDTH - 16} wrap={false} />
        </div>
      ))}
    </div>
  )
}

interface BarViewProps {
  row: Row
  left: number
  width: number
  /** Where the visible axis starts in the scroller: a bar that starts before it keeps its title in view. */
  labelLeft: number
  editable: boolean
  dragging: boolean
  onPointerDown: (event: ReactPointerEvent<HTMLElement>, edge: BarEdge) => void
  onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void
  onPointerUp: () => void
  onPointerCancel: () => void
}

/** A row's bar: the middle moves it, the edges resize it; a click opens the row. */
function BarView({ row, left, width, labelLeft, editable, dragging, onPointerDown, onPointerMove, onPointerUp, onPointerCancel }: BarViewProps) {
  const label = row.title || 'Untitled'
  const inside = width >= LABEL_INSIDE_MIN
  const resizable = editable && width >= RESIZE_MIN
  const handlers = { onPointerMove, onPointerUp, onPointerCancel }
  return (
    <>
      <div
        role="button"
        tabIndex={0}
        aria-label={label}
        className={cn(
          'absolute top-1.5 flex items-center overflow-clip rounded-sm border border-primary/40 bg-primary/15 text-xs text-foreground outline-none select-none hover:bg-primary/25 focus-visible:ring-2 focus-visible:ring-ring/50',
          editable ? 'cursor-grab' : 'cursor-pointer',
          dragging && 'cursor-grabbing bg-primary/30 shadow-sm',
        )}
        style={{ left, width, height: ROW_HEIGHT - 12, touchAction: 'none' }}
        onPointerDown={(event) => onPointerDown(event, 'both')}
        onClick={(event) => event.stopPropagation()}
        {...handlers}
      >
        {resizable ? <EdgeHandle side="start" onPointerDown={(event) => onPointerDown(event, 'start')} {...handlers} /> : null}
        {inside ? (
          // overflow-clip on the bar (not hidden) keeps the timeline, not the bar, the scroller this title sticks to.
          <span className="sticky truncate px-2" style={{ left: labelLeft }}>
            {label}
          </span>
        ) : null}
        {resizable ? <EdgeHandle side="end" onPointerDown={(event) => onPointerDown(event, 'end')} {...handlers} /> : null}
      </div>
      {inside ? null : (
        <span className="pointer-events-none absolute top-1.5 flex items-center text-xs whitespace-nowrap text-muted-foreground" style={{ left: left + width + 6, height: ROW_HEIGHT - 12 }}>
          {label}
        </span>
      )}
    </>
  )
}

function EdgeHandle({ side, ...handlers }: { side: 'start' | 'end'; onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void; onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void; onPointerUp: () => void; onPointerCancel: () => void }) {
  return (
    <span
      aria-hidden="true"
      className={cn('absolute inset-y-0 z-10 cursor-ew-resize hover:bg-primary/30', side === 'start' ? 'left-0' : 'right-0')}
      style={{ width: EDGE_WIDTH }}
      {...handlers}
    />
  )
}

/** The timeline entries of the view options menu: start and end date properties, the table pane and its properties. */
export function TimelineMenus({ properties, config, onConfig }: { properties: Property[]; config: ViewConfig; onConfig: (config: ViewConfig) => void }) {
  const timeline = timelineOf(config, properties)
  const dates = properties.filter((property) => property.type === 'date')
  const shown = new Set(timeline.columns.map((property) => property.id))
  return (
    <>
      <DateMenu
        label="Start date"
        icon={<CalendarDate />}
        choices={dates}
        value={timeline.start?.id ?? null}
        onChange={(id) => onConfig({ ...config, start_property_id: id ?? undefined, ...(id === config.end_property_id ? { end_property_id: undefined } : {}) })}
      />
      <DateMenu
        label="End date"
        icon={<Calendar />}
        choices={dates.filter((property) => property.id !== timeline.start?.id)}
        value={timeline.end?.id ?? null}
        onChange={(id) => onConfig({ ...config, end_property_id: id ?? undefined })}
        none
      />
      <DropdownMenuCheckboxItem checked={timeline.showTable} onCheckedChange={(checked) => onConfig({ ...config, show_table: checked })}>
        <SidebarLeft />
        Show table
      </DropdownMenuCheckboxItem>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger className="whitespace-nowrap" disabled={!timeline.showTable}>
          <Eye />
          Table properties
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent className="max-h-80 w-52 overflow-y-auto">
          {properties
            .filter((property) => property.type !== 'title')
            .map((property) => {
              const Icon = PROPERTY_TYPES[property.type].icon
              return (
                <DropdownMenuCheckboxItem
                  key={property.id}
                  checked={shown.has(property.id)}
                  onCheckedChange={(checked) =>
                    onConfig({
                      ...config,
                      table_columns: checked ? [...(config.table_columns ?? []), property.id] : (config.table_columns ?? []).filter((id) => id !== property.id),
                    })
                  }
                >
                  <Icon className="text-muted-foreground" />
                  <span className="truncate">{property.name}</span>
                </DropdownMenuCheckboxItem>
              )
            })}
        </DropdownMenuSubContent>
      </DropdownMenuSub>
    </>
  )
}

function DateMenu({ label, icon, choices, value, onChange, none = false }: { label: string; icon: ReactNode; choices: Property[]; value: string | null; onChange: (id: string | null) => void; none?: boolean }) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger className="whitespace-nowrap" disabled={choices.length === 0 && !none}>
        {icon}
        {label}
        <span className="ml-auto max-w-20 min-w-0 truncate text-xs text-muted-foreground">{choices.find((property) => property.id === value)?.name ?? (none ? 'None' : '')}</span>
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
          {choices.map((property) => (
            <DropdownMenuRadioItem key={property.id} value={property.id}>
              <CalendarDate className="text-muted-foreground" />
              <span className="truncate">{property.name}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}
