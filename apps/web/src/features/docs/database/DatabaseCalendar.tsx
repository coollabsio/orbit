import { useRef, useState, type DragEvent } from 'react'
import { Add, ChevronLeft, ChevronRight } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  MAX_DAY_CARDS,
  barsOn,
  calendarWeeks,
  datable,
  dayDate,
  dayDiff,
  moveDate,
  monthTitle,
  placeRows,
  startTime,
  undatedRows,
  weekOf,
  weekdayName,
  withEndDay,
  type CalendarBar,
} from '@orbit/database/calendar'
import { addDays, addMonths, dateText, localDay } from '@orbit/database/dates'
import type { Database, DateProperty, Property, Row, ViewConfig } from '@orbit/database/types'
import { apiValue, cellOf, filledProperties } from '@orbit/database/values'
import { useCreateProperty, useUpdateValues } from './api'
import { CellView } from './cells/CellView'
import { useDatabaseEnv } from './context'
import { useWeekStart } from './weekStart'

const DAY_HEADER = 26
const MONTH_LANE = 24
const WEEK_LANE_BASE = 28
const WEEK_LANE_PROPERTY = 20
/** Card properties a week card shows under its title. */
const WEEK_CARD_PROPERTIES = 3
const MORE_HEIGHT = 20

interface DatabaseCalendarProps {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched and sorted). */
  rows: Row[]
  onConfig: (config: ViewConfig) => void
  /** "+" on a day: a new row with `values` (the day's date). */
  onAddRow: (values: Record<string, unknown>) => void
  onOpenRow: (rowId: string) => void
}

/** A card drag: `move` shifts the whole date by the days the pointer moves; `end` sets a range's last day. */
type CalendarDrag = { rowId: string; mode: 'move'; grabDay: string } | { rowId: string; mode: 'end' }

/**
 * A calendar view (spec P3 13.2): the rows on a month grid (6 weeks) or one week by the view's date property. A range
 * is a bar across its days; a month cell shows 3 cards, then "+N more". Cards of a `date` property drag to another
 * day (start and end move together, times stay) and a range's right edge drags to change the end. "+" on a day adds
 * a row with that date; rows with no date are under "No date (N)".
 */
export function DatabaseCalendar({ database, config, rows, onConfig, onAddRow, onOpenRow }: DatabaseCalendarProps) {
  const { workspaceId, databaseId, canEditValues, canEditSchema, canEditViews, ctx } = useDatabaseEnv()
  const updateValues = useUpdateValues(workspaceId, databaseId)
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const weekStart = useWeekStart(ctx.locale)
  const today = localDay(ctx.now, ctx.timeZone)
  const [anchor, setAnchor] = useState(today)
  const [drag, setDrag] = useState<CalendarDrag | null>(null)
  const [dropDay, setDropDay] = useState<string | null>(null)
  const dragTimer = useRef<number | undefined>(undefined)

  const candidate = database.properties.find((property) => property.id === config.date_property_id)
  if (!datable(candidate)) {
    const choices = database.properties.filter(datable)
    return (
      <div className="flex h-40 flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
        Choose a date property
        <span className="text-xs">A calendar places its rows by a Date, Created time or Last edited time property.</span>
        {canEditViews && choices.length > 0 ? (
          <div className="flex flex-wrap justify-center gap-1.5">
            {choices.map((property) => (
              <Button key={property.id} type="button" variant="outline" size="sm" onClick={() => onConfig({ ...config, date_property_id: property.id })}>
                {property.name}
              </Button>
            ))}
          </div>
        ) : canEditSchema ? (
          <Button type="button" variant="outline" size="sm" disabled={createProperty.isPending} onClick={() => createProperty.mutate({ name: 'Date', type: 'date' })}>
            <Add />
            Add a Date property
          </Button>
        ) : null}
      </div>
    )
  }
  const property = candidate
  const week = config.calendar_mode === 'week'
  const weeks = week ? [weekOf(anchor, weekStart)] : calendarWeeks(anchor, weekStart)
  const month = anchor.slice(0, 7)
  const undated = undatedRows(rows, property, ctx.timeZone)
  const movable = canEditValues && property.type === 'date'
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((other) => other.id === id) ?? []).slice(0, week ? WEEK_CARD_PROPERTIES : 0)
  const laneHeight = week ? WEEK_LANE_BASE + WEEK_LANE_PROPERTY * cardProperties.length : MONTH_LANE
  const step = (direction: 1 | -1) => setAnchor((day) => (week ? addDays(day, 7 * direction) : addMonths(day, direction)))

  const beginDrag = (event: DragEvent<HTMLElement>, row: Row, next: CalendarDrag) => {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', row.title || 'Untitled')
    // Chrome cancels a drag whose source stops taking pointer events during `dragstart`, so the cards let the drag
    // through to the day cells only from the next task on.
    dragTimer.current = window.setTimeout(() => setDrag(next))
  }
  const endDrag = () => {
    window.clearTimeout(dragTimer.current)
    setDrag(null)
    setDropDay(null)
  }
  const dropOn = (day: string) => {
    const current = drag
    endDrag()
    const row = current && rows.find((candidateRow) => candidateRow.id === current.rowId)
    const value = row && cellOf(row, property)
    if (!current || value?.kind !== 'date') return
    const date = current.mode === 'move' ? moveDate(value.date, dayDiff(current.grabDay, day), ctx.timeZone) : withEndDay(value.date, day, ctx.timeZone)
    if (!date || dateText(date) === dateText(value.date)) return
    updateValues.mutate({ rowId: current.rowId, values: { [property.id]: apiValue({ kind: 'date', date }) } })
  }
  const dayZone = (day: string) => ({
    onDragOver: (event: DragEvent<HTMLElement>) => {
      if (!drag) return
      event.preventDefault()
      event.dataTransfer.dropEffect = 'move'
      if (dropDay !== day) setDropDay(day)
    },
    onDrop: (event: DragEvent<HTMLElement>) => {
      event.preventDefault()
      dropOn(day)
    },
  })

  // `contain` keeps the toolbar's width out of the page's `w-max` column, so a phone fits the 7 days instead of scrolling.
  return (
    <div className="flex flex-col gap-2 pb-4 text-sm [contain:inline-size]">
      <div className="flex flex-wrap items-center gap-1">
        <h3 className="mr-2 text-base font-semibold">{monthTitle(week ? weeks[0]![0]! : anchor, ctx.locale)}</h3>
        <Button type="button" variant="ghost" size="icon-sm" aria-label={week ? 'Previous week' : 'Previous month'} onClick={() => step(-1)}>
          <ChevronLeft />
        </Button>
        <Button type="button" variant="ghost" size="sm" onClick={() => setAnchor(today)}>
          Today
        </Button>
        <Button type="button" variant="ghost" size="icon-sm" aria-label={week ? 'Next week' : 'Next month'} onClick={() => step(1)}>
          <ChevronRight />
        </Button>
        <span className="flex-1" />
        {undated.length > 0 ? <RowListPopover label={`No date (${undated.length})`} rows={undated} title={database.properties.find((other) => other.type === 'title')} onOpenRow={onOpenRow} /> : null}
        <div role="radiogroup" aria-label="Calendar range" className="flex rounded-md border border-border p-0.5">
          {(['month', 'week'] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              role="radio"
              aria-checked={(config.calendar_mode ?? 'month') === mode}
              className="rounded px-2 py-0.5 text-xs text-muted-foreground aria-checked:bg-muted aria-checked:text-foreground"
              onClick={() => {
                const { calendar_mode: _mode, ...rest } = config
                onConfig(mode === 'week' ? { ...rest, calendar_mode: 'week' } : rest)
              }}
            >
              {mode === 'month' ? 'Month' : 'Week'}
            </button>
          ))}
        </div>
      </div>
      <div className="overflow-hidden rounded-md border border-border" aria-busy={updateValues.isPending} data-dragging={drag ? '' : undefined}>
        <div className="grid grid-cols-7 border-b border-border text-xs text-muted-foreground">
          {weeks[0]!.map((day) => (
            <div key={day} className="px-2 py-1">
              {weekdayName(day, ctx.locale)}
            </div>
          ))}
        </div>
        {weeks.map((days) => {
          const bars = placeRows(rows, property, days, ctx.timeZone)
          const maxLanes = week ? Infinity : MAX_DAY_CARDS
          const lanes = Math.max(0, ...bars.map((bar) => bar.lane + 1))
          const height = DAY_HEADER + (week ? Math.max(lanes, 8) * laneHeight : MAX_DAY_CARDS * MONTH_LANE + MORE_HEIGHT) + 4
          return (
            <div key={days[0]} data-week="" className="relative grid grid-cols-7 border-b border-border last:border-b-0" style={{ height }}>
              {days.map((day, column) => {
                const hidden = barsOn(bars, column).filter((bar) => bar.lane >= maxLanes)
                return (
                  <div
                    key={day}
                    className={cn(
                      'group/day relative border-r border-border last:border-r-0',
                      !week && day.slice(0, 7) !== month && 'bg-muted/30 text-muted-foreground',
                      dropDay === day && 'bg-primary/10',
                    )}
                    {...dayZone(day)}
                  >
                    <div className="flex h-[26px] items-center justify-between px-1.5">
                      {canEditValues && property.type === 'date' ? (
                        <button
                          type="button"
                          aria-label={`New row on ${day}`}
                          className="flex size-5 items-center justify-center rounded text-muted-foreground opacity-0 group-hover/day:opacity-100 hover:bg-muted focus-visible:opacity-100"
                          onClick={() => onAddRow({ [property.id]: apiValue({ kind: 'date', date: dayDate(day) }) })}
                        >
                          <Add className="size-3.5" />
                        </button>
                      ) : (
                        <span />
                      )}
                      <span className={cn('flex h-5 min-w-5 items-center justify-center rounded-full px-1 text-xs tabular-nums', day === today && 'bg-primary text-primary-foreground')}>
                        {Number(day.slice(8))}
                      </span>
                    </div>
                    {hidden.length > 0 ? (
                      <div className="absolute inset-x-1 bottom-0.5">
                        <RowListPopover
                          label={`+${hidden.length} more`}
                          rows={barsOn(bars, column).map((bar) => bar.row)}
                          title={database.properties.find((other) => other.type === 'title')}
                          onOpenRow={onOpenRow}
                          compact
                        />
                      </div>
                    ) : null}
                  </div>
                )
              })}
              {bars
                .filter((bar) => bar.lane < maxLanes)
                .map((bar) => (
                  <CalendarCard
                    key={bar.row.id}
                    bar={bar}
                    property={property}
                    cardProperties={cardProperties}
                    top={DAY_HEADER + bar.lane * laneHeight}
                    height={laneHeight - 2}
                    draggable={movable}
                    dragging={drag?.rowId === bar.row.id}
                    passThrough={drag !== null}
                    onOpen={() => onOpenRow(bar.row.id)}
                    onMoveStart={(event) => {
                      const box = (event.currentTarget.closest('[data-week]') as HTMLElement).getBoundingClientRect()
                      const column = Math.min(6, Math.max(0, Math.floor(((event.clientX - box.left) / box.width) * 7)))
                      beginDrag(event, bar.row, { rowId: bar.row.id, mode: 'move', grabDay: days[column]! })
                    }}
                    onEndStart={(event) => {
                      event.stopPropagation()
                      beginDrag(event, bar.row, { rowId: bar.row.id, mode: 'end' })
                    }}
                    onDragEnd={endDrag}
                  />
                ))}
            </div>
          )
        })}
      </div>
    </div>
  )
}

interface CalendarCardProps {
  bar: CalendarBar
  property: DateProperty
  cardProperties: Property[]
  top: number
  height: number
  draggable: boolean
  dragging: boolean
  /** While a card drags, cards let the drag reach the day cells under them. */
  passThrough: boolean
  onOpen: () => void
  onMoveStart: (event: DragEvent<HTMLElement>) => void
  onEndStart: (event: DragEvent<HTMLElement>) => void
  onDragEnd: () => void
}

/** A row on the grid: its title (with the start time) across its days of the week, and in a week its card properties. */
function CalendarCard({ bar, property, cardProperties, top, height, draggable, dragging, passThrough, onOpen, onMoveStart, onEndStart, onDragEnd }: CalendarCardProps) {
  const { ctx } = useDatabaseEnv()
  const { row } = bar
  const time = startTime(row, property, ctx)
  const filled = filledProperties(row, cardProperties)
  return (
    <article
      data-dragging={dragging || undefined}
      draggable={draggable}
      tabIndex={0}
      className={cn(
        'absolute flex cursor-pointer flex-col gap-0.5 overflow-hidden border border-border bg-card px-1.5 text-xs shadow-xs outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 data-dragging:opacity-35',
        bar.continuesBefore ? 'rounded-l-none border-l-0' : 'rounded-l-md',
        bar.continuesAfter ? 'rounded-r-none border-r-0' : 'rounded-r-md',
        passThrough && 'pointer-events-none',
      )}
      style={{ top, height, left: `calc(${(bar.column / 7) * 100}% + 3px)`, width: `calc(${(bar.length / 7) * 100}% - 6px)` }}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget && event.key === 'Enter') onOpen()
      }}
      onDragStart={onMoveStart}
      onDragEnd={onDragEnd}
    >
      <div className="flex h-[22px] min-w-0 shrink-0 items-center gap-1">
        {time ? <span className="shrink-0 text-muted-foreground tabular-nums">{time}</span> : null}
        <span className="min-w-0 flex-1 truncate font-medium">{row.title || <span className="text-muted-foreground">Untitled</span>}</span>
      </div>
      {filled.map((other) => (
        <div key={other.id} className="flex h-[18px] min-w-0 items-center">
          <CellView property={other} row={row} width={160} wrap={false} />
        </div>
      ))}
      {draggable && !bar.continuesAfter && bar.span.end !== bar.span.start ? (
        <span
          draggable
          role="separator"
          aria-label="Drag to change the end date"
          className="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize"
          onClick={(event) => event.stopPropagation()}
          onDragStart={onEndStart}
          onDragEnd={onDragEnd}
        />
      ) : null}
    </article>
  )
}

/** A button that lists rows in a popover (a day's "+N more", "No date (N)"); a row opens its peek. */
function RowListPopover({ label, rows, title, onOpenRow, compact = false }: { label: string; rows: Row[]; title: Property | undefined; onOpenRow: (rowId: string) => void; compact?: boolean }) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          compact ? (
            <button type="button" className="h-[18px] w-full truncate rounded px-1 text-left text-xs text-muted-foreground hover:bg-muted" />
          ) : (
            <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" />
          )
        }
      >
        {label}
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 gap-0 p-1">
        <ul className="flex max-h-80 flex-col overflow-y-auto">
          {rows.map((row) => (
            <li key={row.id}>
              <button
                type="button"
                className="flex h-8 w-full items-center rounded-md px-2 text-left text-sm hover:bg-muted/60"
                onClick={() => {
                  setOpen(false)
                  onOpenRow(row.id)
                }}
              >
                {title ? <CellView property={title} row={row} width={220} wrap={false} /> : row.title || 'Untitled'}
              </button>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  )
}
