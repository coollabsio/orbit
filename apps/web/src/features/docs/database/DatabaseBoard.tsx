import { useState, type DragEvent, type ReactNode } from 'react'
import { Add, ChevronDown, ChevronRight, Eye, EyeSlash, MoreH } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { indexAt } from '@/features/views/useGroupDrop'
import { cardAfter, groupName, groupRows, groupValues, laneRows, moveValues, type BoardGroup } from '@orbit/database/board'
import type { Database, LaneProperty, Property, Row, ViewConfig } from '@orbit/database/types'
import { filledProperties } from '@orbit/database/values'
import { groupable, laneable, withGroupBy } from '@orbit/database/view'
import { useCreateProperty, useMoveRow, useUpdateRowTitle, useUpdateValues } from './api'
import { CellView, OptionPill } from './cells/CellView'
import { useDatabaseEnv } from './context'
import { PROPERTY_TYPES } from './propertyMeta'

const COLUMN_WIDTH = 260
/** Cards each column renders before "Show more"; a column can hold all 5,000 rows. */
const PAGE = 50
/** Cards each cell of a board with lanes renders before "Show more" (spec P2 8.5). */
const LANE_PAGE = 25

export type NewCard = { values: Record<string, unknown>; after?: string | null; at: 'top' | 'bottom' }

interface DatabaseBoardProps {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched, sorted, with new rows pinned). */
  rows: Row[]
  onConfig: (config: ViewConfig) => void
  onAddCard: (card: NewCard) => void
  onOpenRow: (rowId: string) => void
  filtered: boolean
  onClearFilters: () => void
  /** A new card whose title field opens. */
  editRowId: string | null
  onEditRowShown: () => void
  /** Cards a column shows at first and adds with "Show more" (default 50). */
  pageSize?: number
  /** Keys the collapsed lanes this browser remembers. */
  viewId?: string
}

/** Where a card sits: its column, and its lane (`null` on a board without lanes). */
type Cell = { group: string | null; lane: string | null }
type CardDrag = { rowId: string; from: Cell }
/** `index`: the slot among the cell's other cards; `null` when the view has sorts (only the values change). */
type CardDrop = { cell: Cell; index: number | null }

const sameCell = (a: Cell, b: Cell) => a.group === b.group && a.lane === b.lane
const cellKey = (cell: Cell) => JSON.stringify([cell.group, cell.lane])

/**
 * A board view (spec 7.7, P2 8): one column per group of the group property (a card with several people or options
 * shows in each of their columns), cards with the title and the card properties, and optional lanes: the groups of
 * the sub-group property, each a band with the same columns. Cards drag between cells (sets the column's and the
 * lane's value in one request) and, without sorts, inside a cell (manual order); column headers drag to reorder the
 * groups. Drag and drop follows the task board: native events, the dragged element stays mounted (faded) and a
 * placeholder shows among the others.
 */
export function DatabaseBoard(props: DatabaseBoardProps) {
  const { database, config, rows, onConfig, onAddCard, onOpenRow, filtered, onClearFilters, editRowId, onEditRowShown, pageSize = PAGE, viewId = '' } = props
  const { workspaceId, databaseId, canEditValues, canEditSchema, ctx } = useDatabaseEnv()
  const moveRow = useMoveRow(workspaceId, databaseId)
  const updateValues = useUpdateValues(workspaceId, databaseId)
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const [cardDrag, setCardDrag] = useState<CardDrag | null>(null)
  const [cardDrop, setCardDrop] = useState<CardDrop | null>(null)
  const [cardHeight, setCardHeight] = useState(0)
  const [columnDrag, setColumnDrag] = useState<{ id: string | null } | null>(null)
  const [columnDrop, setColumnDrop] = useState<number | null>(null)
  const [shown, setShown] = useState<Readonly<Record<string, number>>>({})
  const [collapsed, setCollapsed] = useCollapsedLanes(`orbit:board-lanes:${databaseId}:${viewId}`)

  const property = database.properties.find((candidate) => candidate.id === config.group_by)
  if (!groupable(property)) {
    const choices = database.properties.filter(groupable)
    return (
      <div className="flex min-h-40 flex-col items-center justify-center gap-2 py-6 text-sm text-muted-foreground">
        Choose a property to group by
        <span className="text-xs">A board groups its cards by a Status, Select, Multi-select or Person property.</span>
        <div className="flex max-w-xl flex-wrap justify-center gap-1.5">
          {choices.map((choice) => {
            const Icon = PROPERTY_TYPES[choice.type].icon
            return (
              <Button key={choice.id} type="button" variant="outline" size="sm" onClick={() => onConfig(withGroupBy(config, choice.id))}>
                <Icon />
                <span className="max-w-40 truncate">{choice.name}</span>
              </Button>
            )
          })}
          {canEditSchema ? (
            <Button type="button" variant="outline" size="sm" disabled={createProperty.isPending} onClick={() => createProperty.mutate({ name: 'Status', type: 'status' })}>
              <Add />
              Add a Status property
            </Button>
          ) : null}
        </div>
      </div>
    )
  }
  const candidate = database.properties.find((other) => other.id === config.sub_group_by)
  const laneProperty = laneable(candidate) ? candidate : undefined
  const sorted = config.sorts.length > 0
  const groups = groupRows(rows, property, config, ctx.members)
  const visible = groups.filter((group) => !group.hidden)
  const hidden = groups.filter((group) => group.hidden)
  const lanes = laneProperty ? laneRows(rows, property, laneProperty, config, ctx.members) : []
  const hiddenLanes = lanes.filter((lane) => lane.hidden)
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((other) => other.id === id) ?? [])
  const title = database.properties.find((other) => other.type === 'title')
  const ordered = [...database.rows].sort((a, b) => a.position - b.position || a.id.localeCompare(b.id))
  const name = (owner: LaneProperty, group: BoardGroup) => groupName(owner, group, ctx.members)
  const cellName = (group: BoardGroup, lane: BoardGroup | null) => (lane && laneProperty ? `${name(property, group)}, ${name(laneProperty, lane)}` : name(property, group))
  const newCardValues = (cell: Cell) => ({ ...groupValues(property, cell.group), ...(laneProperty ? groupValues(laneProperty, cell.lane) : {}) })

  const endCardDrag = () => {
    setCardDrag(null)
    setCardDrop(null)
  }
  const dropCard = (cell: Cell, cellRows: readonly Row[], index: number | null) => {
    const drag = cardDrag
    endCardDrag()
    const row = drag && rows.find((candidate) => candidate.id === drag.rowId)
    if (!drag || !row) return
    const patch = { ...moveValues(row, property, drag.from.group, cell.group), ...(laneProperty ? moveValues(row, laneProperty, drag.from.lane, cell.lane) : {}) }
    const values = Object.keys(patch).length > 0 ? patch : undefined
    if (index === null) {
      if (values) updateValues.mutate({ rowId: drag.rowId, values })
      return
    }
    const after = cardAfter(ordered, cellRows.filter((other) => other.id !== drag.rowId), index, drag.rowId)
    if (!values && after === cardAfter(ordered, [], 0, drag.rowId)) return
    moveRow.mutate({ rowId: drag.rowId, afterRowId: after, values })
  }
  const cardZone = (cell: Cell, cellRows: readonly Row[]) => ({
    onDragOver: (event: DragEvent<HTMLElement>) => {
      if (!cardDrag || (sorted && sameCell(cardDrag.from, cell))) return
      event.preventDefault()
      event.dataTransfer.dropEffect = 'move'
      const index = sorted ? null : indexAt(event.currentTarget, '[data-board-card]', event.clientY)
      if (!cardDrop || !sameCell(cardDrop.cell, cell) || cardDrop.index !== index) setCardDrop({ cell, index })
    },
    onDragLeave: (event: DragEvent<HTMLElement>) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null) && cardDrop && sameCell(cardDrop.cell, cell)) setCardDrop(null)
    },
    onDrop: (event: DragEvent<HTMLElement>) => {
      event.preventDefault()
      dropCard(cell, cellRows, sorted ? null : indexAt(event.currentTarget, '[data-board-card]', event.clientY))
    },
  })

  const endColumnDrag = () => {
    setColumnDrag(null)
    setColumnDrop(null)
  }
  const columnSlot = (board: HTMLElement, clientX: number) => {
    const columns = Array.from(board.querySelectorAll<HTMLElement>('[data-board-column]:not([data-dragging])'))
    const right = columns.findIndex((column) => {
      const rect = column.getBoundingClientRect()
      return clientX < rect.left + rect.width / 2
    })
    return right === -1 ? columns.length : right
  }
  const dropColumn = (slot: number) => {
    const drag = columnDrag
    endColumnDrag()
    if (!drag) return
    const ids = visible.map((group) => group.id).filter((id) => id !== drag.id)
    ids.splice(slot, 0, drag.id)
    onConfig({ ...config, group_order: [...ids, ...hidden.map((group) => group.id)] })
  }
  const columnZone = {
    onDragOver: (event: DragEvent<HTMLElement>) => {
      if (!columnDrag) return
      event.preventDefault()
      event.dataTransfer.dropEffect = 'move'
      const slot = columnSlot(event.currentTarget, event.clientX)
      if (slot !== columnDrop) setColumnDrop(slot)
    },
    onDrop: (event: DragEvent<HTMLElement>) => {
      if (!columnDrag) return
      event.preventDefault()
      dropColumn(columnSlot(event.currentTarget, event.clientX))
    },
  }
  const setHidden = (group: BoardGroup, hide: boolean) =>
    onConfig({ ...config, hidden_groups: hide ? [...config.hidden_groups, group.id] : config.hidden_groups.filter((id) => id !== group.id) })
  const setLaneHidden = (lane: BoardGroup, hide: boolean) =>
    onConfig({ ...config, hidden_sub_groups: hide ? [...config.hidden_sub_groups, lane.id] : config.hidden_sub_groups.filter((id) => id !== lane.id) })

  const othersOf = (columnId: string | null) => (columnDrag ? visible.filter((group) => group.id !== columnId) : visible)
  const columnPlaceholder = <div aria-hidden="true" className="w-1 shrink-0 self-stretch rounded-full bg-primary/40" />

  /** A column's header: the group, its card count, "Hide group" and "Add a card"; it drags to reorder the columns. */
  const header = (group: BoardGroup) => (
    <header
      draggable
      className="group/header flex h-9 cursor-grab items-center gap-1.5 px-1"
      onDragStart={(event) => {
        event.dataTransfer.effectAllowed = 'move'
        event.dataTransfer.setData('text/plain', name(property, group))
        setColumnDrag({ id: group.id })
      }}
      onDragEnd={endColumnDrag}
    >
      <GroupLabel property={property} group={group} />
      <span className="text-xs text-muted-foreground tabular-nums">{group.rows.length}</span>
      <span className="flex-1" />
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label={`${name(property, group)} group options`}
              className="text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/header:opacity-100 aria-expanded:opacity-100"
            />
          }
        >
          <MoreH />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-40">
          <DropdownMenuItem onClick={() => setHidden(group, true)}>
            <EyeSlash />
            Hide group
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {canEditValues && !laneProperty ? (
        <Tip label="Add a card">
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`Add a card to ${name(property, group)}`}
            className="text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/header:opacity-100"
            onClick={() => onAddCard({ values: newCardValues({ group: group.id, lane: null }), after: cardAfter(ordered, group.rows, 0, ''), at: 'top' })}
          >
            <Add />
          </Button>
        </Tip>
      ) : null}
    </header>
  )

  /** The cards of one cell (a column, or a column in a lane), the drop placeholder, "Show more" and "New". */
  const cards = (cell: Cell, cellRows: readonly Row[], page: number) => {
    const key = cellKey(cell)
    const limit = shown[key] ?? page
    const list = cellRows.slice(0, limit)
    const dragged = (row: Row) => cardDrag?.rowId === row.id && sameCell(cardDrag.from, cell)
    const others = cardDrag ? list.filter((row) => !dragged(row)) : list
    const placeholderIndex = cardDrop && sameCell(cardDrop.cell, cell) ? cardDrop.index : null
    const placeholder = <div data-board-placeholder className="min-h-10 rounded-md border border-dashed border-primary/40 bg-primary/10" style={{ height: cardHeight }} />
    return (
      <>
        {sorted && cardDrag && sameCell(cardDrag.from, cell) ? <p className="px-1 pb-1 text-xs text-muted-foreground">Remove sorts to reorder cards</p> : null}
        <div className="flex min-h-10 flex-col gap-1.5 px-0.5 pb-1">
          {list.map((row) => (
            <div key={row.id} className="contents">
              {!dragged(row) && placeholderIndex !== null && placeholderIndex === others.indexOf(row) ? placeholder : null}
              <Card
                row={row}
                title={title}
                properties={cardProperties}
                dragging={dragged(row)}
                draggable={canEditValues}
                editing={row.id === editRowId}
                onEditShown={onEditRowShown}
                onOpen={() => onOpenRow(row.id)}
                onDragStart={(event) => {
                  event.stopPropagation()
                  event.dataTransfer.effectAllowed = 'move'
                  event.dataTransfer.setData('text/plain', row.title)
                  setCardHeight(event.currentTarget.offsetHeight)
                  setCardDrag({ rowId: row.id, from: cell })
                }}
                onDragEnd={endCardDrag}
              />
            </div>
          ))}
          {placeholderIndex !== null && placeholderIndex >= others.length ? placeholder : null}
          {cellRows.length > limit ? (
            <Button type="button" variant="ghost" size="sm" className="justify-start text-muted-foreground" onClick={() => setShown((current) => ({ ...current, [key]: limit + page }))}>
              Show {Math.min(page, cellRows.length - limit)} more
            </Button>
          ) : null}
          {canEditValues ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className={cn('justify-start text-muted-foreground', laneProperty ? 'hover-fine:opacity-0 hover-fine:group-hover/cell:opacity-100 focus-visible:opacity-100' : '')}
              onClick={() => {
                const last = cellRows.at(-1)
                onAddCard({ values: newCardValues(cell), ...(last ? { after: last.id } : {}), at: 'bottom' })
              }}
            >
              <Add />
              New
            </Button>
          ) : null}
        </div>
      </>
    )
  }

  const cellClass =
    'flex shrink-0 flex-col rounded-md transition-opacity data-dragging:opacity-35 data-drop-over:bg-primary/5 data-drop-over:ring-1 data-drop-over:ring-primary/40'
  const dropOver = (cell: Cell) => (cardDrop !== null && sameCell(cardDrop.cell, cell) && cardDrop.index === null) || undefined
  const withColumnSlots = (render: (group: BoardGroup) => ReactNode) => (
    <>
      {visible.map((group) => {
        const slot = columnDrag ? othersOf(columnDrag.id).indexOf(group) : -1
        return (
          <div key={group.id ?? ''} className="contents">
            {columnDrop !== null && slot === columnDrop ? columnPlaceholder : null}
            {render(group)}
          </div>
        )
      })}
      {columnDrop !== null && columnDrop >= othersOf(columnDrag?.id ?? null).length ? columnPlaceholder : null}
    </>
  )
  const hiddenList = (label: string, owner: LaneProperty, list: BoardGroup[], onShow: (group: BoardGroup) => void) => (
    <section aria-label={label} className="flex w-[220px] shrink-0 flex-col gap-0.5">
      <h3 className="flex h-9 items-center px-1 text-sm text-muted-foreground">{label}</h3>
      {list.map((group) => (
        <div key={group.id ?? ''} className="flex h-8 items-center gap-1.5 rounded-md px-1 hover:bg-muted/60">
          <GroupLabel property={owner} group={group} />
          <span className="text-xs text-muted-foreground tabular-nums">{group.rows.length}</span>
          <span className="flex-1" />
          <Tip label="Show group">
            <Button type="button" variant="ghost" size="icon-xs" aria-label={`Show ${name(owner, group)}`} className="text-muted-foreground" onClick={() => onShow(group)}>
              <Eye />
            </Button>
          </Tip>
        </div>
      ))}
    </section>
  )

  return (
    <div className="flex flex-col gap-2 pb-4">
      {rows.length === 0 && filtered ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          No results
          <Button type="button" variant="outline" size="sm" onClick={onClearFilters}>
            Clear filters
          </Button>
        </div>
      ) : null}
      {laneProperty ? (
        <div className="flex w-max flex-col" aria-busy={moveRow.isPending || updateValues.isPending}>
          <div className="sticky top-0 z-10 flex items-start gap-3 bg-background" {...columnZone}>
            {withColumnSlots((group) => (
              <div
                key={group.id ?? ''}
                role="columnheader"
                aria-label={name(property, group)}
                data-board-column=""
                data-dragging={columnDrag?.id === group.id || undefined}
                className="shrink-0 transition-opacity data-dragging:opacity-35"
                style={{ width: COLUMN_WIDTH }}
              >
                {header(group)}
              </div>
            ))}
            {hidden.length > 0 ? hiddenList('Hidden groups', property, hidden, (group) => setHidden(group, false)) : null}
          </div>
          {lanes
            .filter((lane) => !lane.hidden)
            .map((lane) => {
              const closed = collapsed.includes(lane.id)
              return (
                <section key={lane.id ?? ''} aria-label={name(laneProperty, lane)} className="flex flex-col border-t border-border pt-1 pb-2">
                  <div className="group/lane sticky left-0 flex h-9 w-fit items-center gap-1.5 px-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-xs"
                      aria-label={`${closed ? 'Expand' : 'Collapse'} ${name(laneProperty, lane)}`}
                      aria-expanded={!closed}
                      className="text-muted-foreground"
                      onClick={() => setCollapsed(closed ? collapsed.filter((id) => id !== lane.id) : [...collapsed, lane.id])}
                    >
                      {closed ? <ChevronRight /> : <ChevronDown />}
                    </Button>
                    <GroupLabel property={laneProperty} group={lane} />
                    <span className="text-xs text-muted-foreground tabular-nums">{lane.rows.length}</span>
                    <Tip label="Hide group">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Hide ${name(laneProperty, lane)}`}
                        className="text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/lane:opacity-100"
                        onClick={() => setLaneHidden(lane, true)}
                      >
                        <EyeSlash />
                      </Button>
                    </Tip>
                  </div>
                  {closed ? null : (
                    <div className="flex items-start gap-3">
                      {lane.columns
                        .filter((column) => !column.hidden)
                        .map((column) => {
                          const cell = { group: column.id, lane: lane.id }
                          return (
                            <section
                              key={column.id ?? ''}
                              aria-label={cellName(column, lane)}
                              data-drop-over={dropOver(cell)}
                              className={cn('group/cell', cellClass)}
                              style={{ width: COLUMN_WIDTH }}
                              {...cardZone(cell, column.rows)}
                            >
                              {cards(cell, column.rows, Math.min(pageSize, LANE_PAGE))}
                            </section>
                          )
                        })}
                    </div>
                  )}
                </section>
              )
            })}
          {hiddenLanes.length > 0 ? <div className="pt-2">{hiddenList('Hidden sub-groups', laneProperty, hiddenLanes, (lane) => setLaneHidden(lane, false))}</div> : null}
        </div>
      ) : (
        <div className="flex items-start gap-3" aria-busy={moveRow.isPending || updateValues.isPending} {...columnZone}>
          {withColumnSlots((group) => {
            const cell = { group: group.id, lane: null }
            return (
              <section
                aria-label={name(property, group)}
                data-board-column=""
                data-dragging={columnDrag?.id === group.id || undefined}
                data-drop-over={dropOver(cell)}
                className={cellClass}
                style={{ width: COLUMN_WIDTH }}
                {...cardZone(cell, group.rows)}
              >
                {header(group)}
                {cards(cell, group.rows, pageSize)}
              </section>
            )
          })}
          {hidden.length > 0 ? hiddenList('Hidden groups', property, hidden, (group) => setHidden(group, false)) : null}
        </div>
      )}
    </div>
  )
}

/** The lanes this browser keeps collapsed, as group ids, in `localStorage` under `key`. */
function useCollapsedLanes(key: string): [(string | null)[], (ids: (string | null)[]) => void] {
  const [ids, setIds] = useState<(string | null)[]>(() => {
    try {
      const stored: unknown = JSON.parse(localStorage.getItem(key) ?? '[]')
      return Array.isArray(stored) ? stored.filter((id): id is string | null => id === null || typeof id === 'string') : []
    } catch {
      return []
    }
  })
  const save = (next: (string | null)[]) => {
    setIds(next)
    try {
      if (next.length > 0) localStorage.setItem(key, JSON.stringify(next))
      else localStorage.removeItem(key)
    } catch {
      // Private mode: the lanes stay collapsed until the page reloads.
    }
  }
  return [ids, save]
}

/** A group's label: its option pill, its member, a checkbox, or "No <property>". */
function GroupLabel({ property, group }: { property: LaneProperty; group: BoardGroup }) {
  const { users } = useDatabaseEnv()
  if (group.id === null) return <span className="truncate text-sm text-muted-foreground">No {property.name}</span>
  switch (property.type) {
    case 'person': {
      const user = users.get(group.id)
      return (
        <span className="flex min-w-0 items-center gap-1.5 text-sm">
          <UserAvatar user={user} name={user ? undefined : '?'} size={18} />
          <span className="truncate">{user?.name ?? 'Former member'}</span>
        </span>
      )
    }
    case 'checkbox':
      return (
        <span className="flex min-w-0 items-center gap-1.5 text-sm">
          <Checkbox checked={group.id === 'true'} disabled tabIndex={-1} aria-hidden="true" />
          <span className="truncate">{group.id === 'true' ? 'Checked' : 'Unchecked'}</span>
        </span>
      )
    default:
      return <OptionPill property={property} id={group.id} />
  }
}

interface CardProps {
  row: Row
  title: Property | undefined
  properties: Property[]
  dragging: boolean
  draggable: boolean
  editing: boolean
  onEditShown: () => void
  onOpen: () => void
  onDragStart: (event: DragEvent<HTMLElement>) => void
  onDragEnd: () => void
}

/** A card: the title (an inline field for a new card), then each card property that has a value. */
function Card({ row, title, properties, dragging, draggable, editing, onEditShown, onOpen, onDragStart, onDragEnd }: CardProps) {
  const filled = filledProperties(row, properties)
  return (
    <article
      data-board-card=""
      data-dragging={dragging || undefined}
      draggable={draggable && !editing}
      tabIndex={0}
      className="flex cursor-pointer flex-col gap-1.5 rounded-md border border-border bg-card px-2.5 py-2 text-sm outline-none hover:bg-muted/40 focus-visible:ring-2 focus-visible:ring-ring/50 data-dragging:border-dashed data-dragging:opacity-35"
      onClick={() => {
        if (!editing) onOpen()
      }}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget && event.key === 'Enter') onOpen()
      }}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
    >
      {editing ? <CardTitleInput row={row} onDone={onEditShown} /> : title ? <CellView property={title} row={row} width={COLUMN_WIDTH - 24} wrap /> : null}
      {filled.map((property) => (
        <div key={property.id} className={cn('flex min-w-0 text-[0.8125rem]', property.type === 'checkbox' && '[&>span]:justify-start')}>
          <CellView property={property} row={row} width={COLUMN_WIDTH - 24} wrap={false} />
        </div>
      ))}
    </article>
  )
}

/** The title field of a new card: Enter or leaving saves, Esc keeps "Untitled". */
function CardTitleInput({ row, onDone }: { row: Row; onDone: () => void }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const updateTitle = useUpdateRowTitle(workspaceId, databaseId)
  const [title, setTitle] = useState(row.title)
  const finish = (save: boolean) => {
    const next = title.trim()
    if (save && next !== row.title) updateTitle.mutate({ rowId: row.id, title: next, version: row.version })
    onDone()
  }
  return (
    <input
      autoFocus
      aria-label="Card title"
      placeholder="Untitled"
      className="w-full bg-transparent font-medium outline-none placeholder:text-muted-foreground"
      value={title}
      onClick={(event) => event.stopPropagation()}
      onChange={(event) => setTitle(event.target.value)}
      onBlur={() => finish(true)}
      onKeyDown={(event) => {
        event.stopPropagation()
        if (event.key === 'Enter') finish(true)
        if (event.key === 'Escape') finish(false)
      }}
    />
  )
}
