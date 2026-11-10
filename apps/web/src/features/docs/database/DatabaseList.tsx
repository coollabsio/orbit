import { useState, type DragEvent } from 'react'
import { Add } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import type { Database, Property, Row, ViewConfig } from '@orbit/database/types'
import { filledProperties } from '@orbit/database/values'
import { useMoveRow } from './api'
import { CellView } from './cells/CellView'
import { useDatabaseEnv } from './context'
import { dropAfterRowId } from './databaseLib'

const PROPERTY_WIDTH = 240

interface DatabaseListProps {
  database: Database
  config: ViewConfig
  /** The view's rows (filtered, searched, sorted, with new rows pinned). */
  rows: Row[]
  onAddRow: () => void
  onOpenRow: (rowId: string) => void
  filtered: boolean
  onClearFilters: () => void
}

type RowDrop = { id: string; side: 'before' | 'after' }

/**
 * A list view (spec P3 13.1): one 36 px line per row with its icon and title, then the visible properties that have a
 * value, right-aligned. A click opens the peek; without sorts, rows drag to a new place in the manual order.
 */
export function DatabaseList({ database, config, rows, onAddRow, onOpenRow, filtered, onClearFilters }: DatabaseListProps) {
  const { workspaceId, databaseId, canEditValues } = useDatabaseEnv()
  const moveRow = useMoveRow(workspaceId, databaseId)
  const [dragId, setDragId] = useState<string | null>(null)
  const [drop, setDrop] = useState<RowDrop | null>(null)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const title = database.properties.find((property) => property.type === 'title')
  const shown = config.columns.flatMap((column) => {
    const property = byId.get(column.property_id)
    return property && !column.hidden && property.type !== 'title' ? [property] : []
  })
  const draggable = canEditValues && config.sorts.length === 0

  const endDrag = () => {
    setDragId(null)
    setDrop(null)
  }
  const dropRow = (event: DragEvent) => {
    event.preventDefault()
    const moving = dragId
    const target = drop
    endDrag()
    if (!moving || !target || moving === target.id) return
    moveRow.mutate({ rowId: moving, afterRowId: dropAfterRowId(database.rows, moving, target.id, target.side) })
  }

  return (
    <div className="flex flex-col pb-2 text-sm" aria-busy={moveRow.isPending}>
      <ul aria-label={database.database.title || 'Database'} className="flex flex-col">
        {rows.map((row) => (
          <ListLine
            key={row.id}
            row={row}
            title={title}
            properties={shown}
            draggable={draggable}
            dragging={dragId === row.id}
            drop={drop?.id === row.id ? drop.side : null}
            onOpen={() => onOpenRow(row.id)}
            onDragStart={(event) => {
              setDragId(row.id)
              event.dataTransfer.effectAllowed = 'move'
              event.dataTransfer.setData('text/plain', row.title || 'Untitled')
            }}
            onDragEnd={endDrag}
            onDragOver={(event) => {
              if (!dragId) return
              event.preventDefault()
              event.dataTransfer.dropEffect = 'move'
              const box = event.currentTarget.getBoundingClientRect()
              const side = event.clientY < box.top + box.height / 2 ? 'before' : 'after'
              if (drop?.id !== row.id || drop.side !== side) setDrop({ id: row.id, side })
            }}
            onDrop={dropRow}
          />
        ))}
      </ul>
      {rows.length === 0 ? (
        <div className="flex h-24 flex-col items-center justify-center gap-2 border-b border-border text-muted-foreground">
          {filtered && database.rows.length > 0 ? (
            <>
              No results
              <Button type="button" variant="outline" size="sm" onClick={onClearFilters}>
                Clear filters
              </Button>
            </>
          ) : (
            'No rows yet'
          )}
        </div>
      ) : null}
      {canEditValues ? (
        <button
          type="button"
          className="flex h-9 w-full items-center gap-1.5 rounded-md px-2 text-muted-foreground outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
          onClick={onAddRow}
        >
          <Add className="size-4" />
          New
        </button>
      ) : null}
    </div>
  )
}

interface ListLineProps {
  row: Row
  title: Property | undefined
  properties: Property[]
  draggable: boolean
  dragging: boolean
  /** Where the dragged row would land next to this one. */
  drop: RowDrop['side'] | null
  onOpen: () => void
  onDragStart: (event: DragEvent<HTMLElement>) => void
  onDragEnd: () => void
  onDragOver: (event: DragEvent<HTMLElement>) => void
  onDrop: (event: DragEvent<HTMLElement>) => void
}

function ListLine({ row, title, properties, draggable, dragging, drop, onOpen, onDragStart, onDragEnd, onDragOver, onDrop }: ListLineProps) {
  const filled = filledProperties(row, properties)
  return (
    <li
      data-dragging={dragging || undefined}
      draggable={draggable}
      tabIndex={0}
      className={cn(
        'relative flex h-9 cursor-pointer items-center gap-3 rounded-md px-2 outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 data-dragging:opacity-35',
        drop === 'before' && 'before:absolute before:inset-x-0 before:top-0 before:h-0.5 before:bg-primary',
        drop === 'after' && 'after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-primary',
      )}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget && event.key === 'Enter') onOpen()
      }}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      <div className="flex min-w-0 flex-1">{title ? <CellView property={title} row={row} width={PROPERTY_WIDTH * 2} wrap={false} /> : null}</div>
      {filled.length > 0 ? (
        <div className="flex min-w-0 shrink items-center justify-end gap-3 overflow-hidden text-[13px] text-muted-foreground">
          {filled.map((property) => (
            <div key={property.id} className="flex max-w-60 min-w-0 shrink-0 items-center" title={property.name}>
              <CellView property={property} row={row} width={PROPERTY_WIDTH} wrap={false} />
            </div>
          ))}
        </div>
      ) : null}
    </li>
  )
}
