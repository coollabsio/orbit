import { useState, type DragEvent } from 'react'
import { Add, Eye, EyeSlash, MoreH } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tip } from '@/components/common/Tip'
import { indexAt } from '@/features/views/useGroupDrop'
import { cardAfter, groupRows, type BoardGroup } from '@orbit/database/board'
import type { Database, Property, Row, ViewConfig } from '@orbit/database/types'
import { cellOf } from '@orbit/database/values'
import { groupable } from '@orbit/database/view'
import { useCreateProperty, useMoveRow, useUpdateRowTitle, useUpdateValues } from './api'
import { CellView, OptionPill } from './cells/CellView'
import { useDatabaseEnv } from './context'

const COLUMN_WIDTH = 260
/** Cards each column renders before "Show more"; a column can hold all 5,000 rows. */
const PAGE = 50

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
}

type CardDrag = { rowId: string; from: string | null }
/** `index`: the slot among the column's other cards; `null` when the view has sorts (only the group changes). */
type CardDrop = { group: string | null; index: number | null }

/**
 * A board view (spec 7.7): one column per option of the group property, cards with the title and the card
 * properties. Cards drag between columns (sets the value) and, without sorts, inside a column (manual order); column
 * headers drag to reorder the groups. Drag and drop follows the task board: native events, the dragged element
 * stays mounted (faded) and a placeholder shows among the others.
 */
export function DatabaseBoard(props: DatabaseBoardProps) {
  const { database, config, rows, onConfig, onAddCard, onOpenRow, filtered, onClearFilters, editRowId, onEditRowShown, pageSize = PAGE } = props
  const { workspaceId, databaseId, canEditValues, canEditSchema } = useDatabaseEnv()
  const moveRow = useMoveRow(workspaceId, databaseId)
  const updateValues = useUpdateValues(workspaceId, databaseId)
  const createProperty = useCreateProperty(workspaceId, databaseId)
  const [cardDrag, setCardDrag] = useState<CardDrag | null>(null)
  const [cardDrop, setCardDrop] = useState<CardDrop | null>(null)
  const [cardHeight, setCardHeight] = useState(0)
  const [columnDrag, setColumnDrag] = useState<{ id: string | null } | null>(null)
  const [columnDrop, setColumnDrop] = useState<number | null>(null)
  const [shown, setShown] = useState<Readonly<Record<string, number>>>({})

  const property = database.properties.find((candidate) => candidate.id === config.group_by)
  if (!groupable(property)) {
    return (
      <div className="flex h-40 flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
        Choose a property to group by
        <span className="text-xs">A board groups its cards by a Status or Select property.</span>
        {canEditSchema ? (
          <Button type="button" variant="outline" size="sm" disabled={createProperty.isPending} onClick={() => createProperty.mutate({ name: 'Status', type: 'status' })}>
            <Add />
            Add a Status property
          </Button>
        ) : null}
      </div>
    )
  }
  const sorted = config.sorts.length > 0
  const groups = groupRows(rows, property, config)
  const visible = groups.filter((group) => !group.hidden)
  const hidden = groups.filter((group) => group.hidden)
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((candidate) => candidate.id === id) ?? [])
  const ordered = [...database.rows].sort((a, b) => a.position - b.position || a.id.localeCompare(b.id))
  const groupValue = (group: BoardGroup) => ({ [property.id]: group.id })

  const endCardDrag = () => {
    setCardDrag(null)
    setCardDrop(null)
  }
  const dropCard = (group: BoardGroup, index: number | null) => {
    const drag = cardDrag
    endCardDrag()
    if (!drag) return
    const values = drag.from === group.id ? undefined : groupValue(group)
    if (index === null) {
      if (values) updateValues.mutate({ rowId: drag.rowId, values })
      return
    }
    const after = cardAfter(ordered, group.rows.filter((row) => row.id !== drag.rowId), index, drag.rowId)
    if (!values && after === cardAfter(ordered, [], 0, drag.rowId)) return
    moveRow.mutate({ rowId: drag.rowId, afterRowId: after, values })
  }
  const cardZone = (group: BoardGroup) => ({
    onDragOver: (event: DragEvent<HTMLElement>) => {
      if (!cardDrag || (sorted && cardDrag.from === group.id)) return
      event.preventDefault()
      event.dataTransfer.dropEffect = 'move'
      const index = sorted ? null : indexAt(event.currentTarget, '[data-board-card]', event.clientY)
      if (cardDrop?.group !== group.id || cardDrop.index !== index) setCardDrop({ group: group.id, index })
    },
    onDragLeave: (event: DragEvent<HTMLElement>) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null) && cardDrop?.group === group.id) setCardDrop(null)
    },
    onDrop: (event: DragEvent<HTMLElement>) => {
      event.preventDefault()
      dropCard(group, sorted ? null : indexAt(event.currentTarget, '[data-board-card]', event.clientY))
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
  const setHidden = (group: BoardGroup, hide: boolean) =>
    onConfig({ ...config, hidden_groups: hide ? [...config.hidden_groups, group.id] : config.hidden_groups.filter((id) => id !== group.id) })

  const groupLabel = (group: BoardGroup) =>
    group.id === null ? <span className="truncate text-sm text-muted-foreground">No {property.name}</span> : <OptionPill property={property} id={group.id} />
  const othersOf = (columnId: string | null) => (columnDrag ? visible.filter((group) => group.id !== columnId) : visible)
  const columnPlaceholder = <div aria-hidden="true" className="w-1 shrink-0 self-stretch rounded-full bg-primary/40" />

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
      <div
        className="flex items-start gap-3"
        aria-busy={moveRow.isPending || updateValues.isPending}
        onDragOver={(event) => {
          if (!columnDrag) return
          event.preventDefault()
          event.dataTransfer.dropEffect = 'move'
          const slot = columnSlot(event.currentTarget, event.clientX)
          if (slot !== columnDrop) setColumnDrop(slot)
        }}
        onDrop={(event) => {
          if (!columnDrag) return
          event.preventDefault()
          dropColumn(columnSlot(event.currentTarget, event.clientX))
        }}
      >
        {visible.map((group) => {
          const key = group.id ?? ''
          const slot = columnDrag ? othersOf(columnDrag.id).indexOf(group) : -1
          const limit = shown[key] ?? pageSize
          const cards = group.rows.slice(0, limit)
          const others = cardDrag ? cards.filter((row) => row.id !== cardDrag.rowId) : cards
          const placeholderIndex = cardDrop?.group === group.id ? cardDrop.index : null
          const placeholder = <div data-board-placeholder className="min-h-10 rounded-md border border-dashed border-primary/40 bg-primary/10" style={{ height: cardHeight }} />
          return (
            <div key={key} className="contents">
              {columnDrop !== null && slot === columnDrop ? columnPlaceholder : null}
              <section
                aria-label={group.option?.name ?? `No ${property.name}`}
                data-board-column=""
                data-dragging={columnDrag?.id === group.id || undefined}
                data-drop-over={(cardDrop?.group === group.id && cardDrop.index === null) || undefined}
                className="flex shrink-0 flex-col rounded-md transition-opacity data-dragging:opacity-35 data-drop-over:bg-primary/5 data-drop-over:ring-1 data-drop-over:ring-primary/40"
                style={{ width: COLUMN_WIDTH }}
                {...cardZone(group)}
              >
                <header
                  draggable
                  className="group/header flex h-9 cursor-grab items-center gap-1.5 px-1"
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = 'move'
                    event.dataTransfer.setData('text/plain', group.option?.name ?? `No ${property.name}`)
                    setColumnDrag({ id: group.id })
                  }}
                  onDragEnd={endColumnDrag}
                >
                  {groupLabel(group)}
                  <span className="text-xs text-muted-foreground tabular-nums">{group.rows.length}</span>
                  <span className="flex-1" />
                  <DropdownMenu>
                    <DropdownMenuTrigger
                      render={
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon-xs"
                          aria-label={`${group.option?.name ?? `No ${property.name}`} group options`}
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
                  {canEditValues ? (
                    <Tip label="Add a card">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Add a card to ${group.option?.name ?? `No ${property.name}`}`}
                        className="text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/header:opacity-100"
                        onClick={() => onAddCard({ values: groupValue(group), after: cardAfter(ordered, group.rows, 0, ''), at: 'top' })}
                      >
                        <Add />
                      </Button>
                    </Tip>
                  ) : null}
                </header>
                {sorted && cardDrag?.from === group.id ? <p className="px-1 pb-1 text-xs text-muted-foreground">Remove sorts to reorder cards</p> : null}
                <div className="flex min-h-10 flex-col gap-1.5 px-0.5 pb-1">
                  {cards.map((row) => {
                    const dragged = cardDrag?.rowId === row.id
                    return (
                      <div key={row.id} className="contents">
                        {!dragged && placeholderIndex !== null && placeholderIndex === others.indexOf(row) ? placeholder : null}
                        <Card
                          row={row}
                          title={database.properties.find((candidate) => candidate.type === 'title')}
                          properties={cardProperties}
                          dragging={dragged}
                          draggable={canEditValues}
                          editing={row.id === editRowId}
                          onEditShown={onEditRowShown}
                          onOpen={() => onOpenRow(row.id)}
                          onDragStart={(event) => {
                            event.stopPropagation()
                            event.dataTransfer.effectAllowed = 'move'
                            event.dataTransfer.setData('text/plain', row.title)
                            setCardHeight(event.currentTarget.offsetHeight)
                            setCardDrag({ rowId: row.id, from: group.id })
                          }}
                          onDragEnd={endCardDrag}
                        />
                      </div>
                    )
                  })}
                  {placeholderIndex !== null && placeholderIndex >= others.length ? placeholder : null}
                  {group.rows.length > limit ? (
                    <Button type="button" variant="ghost" size="sm" className="justify-start text-muted-foreground" onClick={() => setShown((current) => ({ ...current, [key]: limit + pageSize }))}>
                      Show {Math.min(pageSize, group.rows.length - limit)} more
                    </Button>
                  ) : null}
                  {canEditValues ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="justify-start text-muted-foreground"
                      onClick={() => {
                        const last = group.rows.at(-1)
                        onAddCard({ values: groupValue(group), ...(last ? { after: last.id } : {}), at: 'bottom' })
                      }}
                    >
                      <Add />
                      New
                    </Button>
                  ) : null}
                </div>
              </section>
            </div>
          )
        })}
        {columnDrop !== null && columnDrop >= othersOf(columnDrag?.id ?? null).length ? columnPlaceholder : null}
        {hidden.length > 0 ? (
          <section aria-label="Hidden groups" className="flex w-[220px] shrink-0 flex-col gap-0.5">
            <h3 className="flex h-9 items-center px-1 text-sm text-muted-foreground">Hidden groups</h3>
            {hidden.map((group) => (
              <div key={group.id ?? ''} className="flex h-8 items-center gap-1.5 rounded-md px-1 hover:bg-muted/60">
                {groupLabel(group)}
                <span className="text-xs text-muted-foreground tabular-nums">{group.rows.length}</span>
                <span className="flex-1" />
                <Tip label="Show group">
                  <Button type="button" variant="ghost" size="icon-xs" aria-label={`Show ${group.option?.name ?? `No ${property.name}`}`} className="text-muted-foreground" onClick={() => setHidden(group, false)}>
                    <Eye />
                  </Button>
                </Tip>
              </div>
            ))}
          </section>
        ) : null}
      </div>
    </div>
  )
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
  const filled = properties.filter((property) => cellOf(row, property) !== null)
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
        <div key={property.id} className={cn('flex min-w-0 text-[13px]', property.type === 'checkbox' && '[&>span]:justify-start')}>
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
