import { useEffect, useLayoutEffect, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent, type PointerEvent } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Add, ArrowUpRight } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Tip } from '@/components/common/Tip'
import { apiValue, cellOf, displayText, numberFromEditText, valueFromText } from '@orbit/database/values'
import type { CellValue, Database, Property, Row, SelectOption, ViewConfig } from '@orbit/database/types'
import { useMoveRow, useTrashRows, useUpdateProperty, useUpdateRowTitle, useUpdateValues } from './api'
import { CellEditor, type EditEnd } from './cells/CellEditor'
import { CellView } from './cells/CellView'
import { AddPropertyButton, ColumnHeaderMenu } from './ColumnHeaderMenu'
import { useDatabaseEnv } from './context'
import { dropAfterRowId } from './databaseLib'
import { GripIcon } from './GripIcon'
import { isComputed } from '@orbit/database/meta'

const ROW_HEIGHT = 33
const MIN_WIDTH = 32
const MAX_WIDTH = 1000
const ADD_COLUMN_WIDTH = 32

type CellKey = { row: string; col: string }
type Column = { property: Property; width: number }

interface DatabaseTableProps {
  database: Database
  config: ViewConfig
  /** The rows the view shows, in order. */
  rows: Row[]
  scrollElement: HTMLElement | null
  onConfig: (config: ViewConfig) => void
  onFilter: (propertyId: string) => void
  onAddRow: () => void
  onOpenRow: (rowId: string) => void
  /** The view has a filter or a search: an empty result offers to clear them. */
  filtered: boolean
  onClearFilters: () => void
  /** A row just added: its title opens for typing once it shows. */
  editRowId: string | null
  onEditRowShown: () => void
}

/** The table view (spec 7.2–7.4): a virtualized grid with a sticky header, cell editing and keyboard moves. */
export function DatabaseTable(props: DatabaseTableProps) {
  const { database, config, rows, scrollElement, onConfig, onFilter, onAddRow, onOpenRow, filtered, onClearFilters, editRowId, onEditRowShown } = props
  const { workspaceId, databaseId, ctx, canEditValues, canEditSchema } = useDatabaseEnv()
  const updateValues = useUpdateValues(workspaceId, databaseId)
  const updateTitle = useUpdateRowTitle(workspaceId, databaseId)
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const moveRow = useMoveRow(workspaceId, databaseId)
  const trashRows = useTrashRows(workspaceId, databaseId)

  const [active, setActive] = useState<CellKey | null>(null)
  const [editing, setEditing] = useState(false)
  /** The cell a popover editor opens from. */
  const [editAnchor, setEditAnchor] = useState<HTMLElement | null>(null)
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [resizing, setResizing] = useState<{ id: string; width: number } | null>(null)
  const [columnDrop, setColumnDrop] = useState<{ id: string; side: 'left' | 'right' } | null>(null)
  const [rowDrop, setRowDrop] = useState<{ id: string; side: 'before' | 'after' } | null>(null)
  const dragging = useRef<{ kind: 'row' | 'column'; id: string } | null>(null)
  /** A pointer is on a resize edge: the header under it must not start a column drag. */
  const resizeHeld = useRef(false)
  const gridRef = useRef<HTMLDivElement>(null)
  const bodyRef = useRef<HTMLDivElement>(null)

  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const columns: Column[] = config.columns.flatMap((column) => {
    const property = byId.get(column.property_id)
    if (!property || column.hidden) return []
    return [{ property, width: resizing?.id === property.id ? resizing.width : column.width }]
  })
  const titleProperty = database.properties.find((property) => property.type === 'title')
  const sorted = config.sorts.length > 0
  const width = columns.reduce((sum, column) => sum + column.width, 0) + ADD_COLUMN_WIDTH
  const rowIndex = new Map(rows.map((row, index) => [row.id, index]))
  const liveSelected = rows.filter((row) => selected.has(row.id))

  // The rows start below the page header, the view toolbar and the table header: the virtualizer needs that offset.
  const [scrollMargin, setScrollMargin] = useState(0)
  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!scrollElement || !body) return
    const measure = () =>
      setScrollMargin(body.getBoundingClientRect().top - scrollElement.getBoundingClientRect().top + scrollElement.scrollTop)
    measure()
    const observer = new ResizeObserver(measure)
    if (scrollElement.firstElementChild) observer.observe(scrollElement.firstElementChild)
    return () => observer.disconnect()
  }, [scrollElement])

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollElement,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: (index) => rows[index].id,
    overscan: 12,
    scrollMargin,
  })

  const editRowIndex = editRowId ? rowIndex.get(editRowId) : undefined
  const titleId = titleProperty?.id
  useEffect(() => {
    if (!editRowId || !titleId || editRowIndex === undefined) return
    setActive({ row: editRowId, col: titleId })
    setEditing(true)
    virtualizer.scrollToIndex(editRowIndex, { align: 'auto' })
    onEditRowShown()
  }, [editRowId, titleId, editRowIndex, virtualizer, onEditRowShown])

  // The active cell keeps the focus (it is the grid's one tab stop) and stays in view.
  useEffect(() => {
    if (!active || editing) return
    const index = rowIndex.get(active.row)
    if (index === undefined) return
    virtualizer.scrollToIndex(index, { align: 'auto' })
    const frame = requestAnimationFrame(() => {
      const cell = gridRef.current?.querySelector<HTMLElement>(`[data-cell="${active.row}:${active.col}"]`)
      cell?.focus({ preventScroll: true })
      cell?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
    })
    return () => cancelAnimationFrame(frame)
    // Only a move of the active cell (or the end of an edit) refocuses it, not every refetch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, editing])

  const editable = (property: Property) => canEditValues && !isComputed(property.type)
  const commit = (row: Row, property: Property, value: CellValue | null) =>
    updateValues.mutate({ rowId: row.id, values: { [property.id]: apiValue(value) } })
  const commitTitle = (row: Row, title: string) => updateTitle.mutate({ rowId: row.id, title: title.slice(0, 500), version: row.version })
  const saveOptions = (property: Property) => (options: SelectOption[]) =>
    updateProperty.mutateAsync({ propertyId: property.id, body: { config: { ...property.config, options } } }).then(() => undefined)
  const toggleCheckbox = (row: Row, property: Property) => commit(row, property, cellOf(row, property) ? null : { kind: 'checked' })

  const move = (rowDelta: number, colDelta: number) => {
    if (!active) return
    let r = rowIndex.get(active.row) ?? 0
    let c = columns.findIndex((column) => column.property.id === active.col)
    c += colDelta
    if (c >= columns.length) {
      c = 0
      r += 1
    } else if (c < 0) {
      c = columns.length - 1
      r -= 1
    }
    r = Math.min(rows.length - 1, Math.max(0, r + rowDelta))
    if (rows[r] && columns[c]) setActive({ row: rows[r].id, col: columns[c].property.id })
  }
  const endEdit = (end: EditEnd) => {
    setEditing(false)
    if (end === 'down') move(1, 0)
    else if (end === 'next') move(0, 1)
    else if (end === 'previous') move(0, -1)
  }

  const activate = (row: Row, property: Property, cell: HTMLElement) => {
    setActive({ row: row.id, col: property.id })
    setEditAnchor(cell)
    setEditing(editable(property) && property.type !== 'checkbox')
  }

  const onGridKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // Keys typed in menus and popovers bubble here through React portals: only a focused cell counts.
    if (!active || editing || !(event.target as HTMLElement).closest('[data-cell]')) return
    const row = rows[rowIndex.get(active.row) ?? -1]
    const property = byId.get(active.col)
    if (!row || !property) return
    const mod = event.metaKey || event.ctrlKey
    const handled = () => event.preventDefault()
    switch (event.key) {
      case 'ArrowUp':
        handled()
        return move(-1, 0)
      case 'ArrowDown':
        handled()
        return move(1, 0)
      case 'ArrowLeft':
        handled()
        return move(0, -1)
      case 'ArrowRight':
        handled()
        return move(0, 1)
      case 'Tab':
        handled()
        return move(0, event.shiftKey ? -1 : 1)
      case 'Enter':
        handled()
        if (property.type === 'checkbox') return editable(property) ? toggleCheckbox(row, property) : undefined
        setEditAnchor((event.target as HTMLElement).closest<HTMLElement>('[data-cell]'))
        return setEditing(editable(property))
      case 'Escape':
        handled()
        setActive(null)
        return (event.target as HTMLElement).blur()
      case ' ':
        handled()
        if (property.type === 'checkbox') return editable(property) ? toggleCheckbox(row, property) : undefined
        return onOpenRow(row.id)
      case 'Delete':
      case 'Backspace':
        handled()
        if (!editable(property)) return
        if (property.type === 'title') return row.title ? commitTitle(row, '') : undefined
        return cellOf(row, property) && property.type !== 'status' ? commit(row, property, null) : undefined
      case 'c':
        if (!mod) return
        handled()
        return void navigator.clipboard.writeText(property.type === 'title' ? row.title : displayText(property, cellOf(row, property), ctx))
    }
  }

  const onGridPaste = (event: ClipboardEvent<HTMLDivElement>) => {
    if (!active || editing || !(event.target as HTMLElement).closest('[data-cell]')) return
    const row = rows[rowIndex.get(active.row) ?? -1]
    const property = byId.get(active.col)
    if (!row || !property || !editable(property)) return
    event.preventDefault()
    const text = event.clipboardData.getData('text/plain')
    if (property.type === 'title') return commitTitle(row, text.trim())
    const number = property.type === 'number' ? numberFromEditText(property, text) : null
    const value = property.type === 'number' ? (number === null ? null : { kind: 'number' as const, number }) : valueFromText(property, text, ctx)
    if (value === null && text.trim() !== '') toast.error(`“${text.trim().slice(0, 40)}” does not fit ${property.name}.`)
    else commit(row, property, value)
  }

  const startResize = (event: PointerEvent<HTMLDivElement>, column: Column) => {
    event.preventDefault()
    event.stopPropagation()
    const startX = event.clientX
    const target = event.currentTarget
    target.setPointerCapture(event.pointerId)
    resizeHeld.current = true
    let next = column.width
    const onMove = (moveEvent: globalThis.PointerEvent) => {
      next = Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, column.width + moveEvent.clientX - startX)))
      setResizing({ id: column.property.id, width: next })
    }
    const onUp = () => {
      resizeHeld.current = false
      target.removeEventListener('pointermove', onMove)
      target.removeEventListener('pointerup', onUp)
      setResizing(null)
      if (next !== column.width) {
        onConfig({ ...config, columns: config.columns.map((other) => (other.property_id === column.property.id ? { ...other, width: next } : other)) })
      }
    }
    target.addEventListener('pointermove', onMove)
    target.addEventListener('pointerup', onUp)
  }

  const dropColumn = (event: DragEvent) => {
    event.preventDefault()
    const source = dragging.current
    setColumnDrop(null)
    if (source?.kind !== 'column' || !columnDrop || source.id === columnDrop.id) return
    const moving = config.columns.find((column) => column.property_id === source.id)
    if (!moving) return
    const rest = config.columns.filter((column) => column.property_id !== source.id)
    const at = rest.findIndex((column) => column.property_id === columnDrop.id)
    rest.splice(columnDrop.side === 'left' ? at : at + 1, 0, moving)
    onConfig({ ...config, columns: rest })
  }
  const dropRow = (event: DragEvent) => {
    event.preventDefault()
    const source = dragging.current
    setRowDrop(null)
    if (source?.kind !== 'row' || !rowDrop || source.id === rowDrop.id) return
    moveRow.mutate({ rowId: source.id, afterRowId: dropAfterRowId(database.rows, source.id, rowDrop.id, rowDrop.side) })
  }

  const toggleSelected = (rowId: string) =>
    setSelected((current) => {
      const next = new Set(current)
      if (!next.delete(rowId)) next.add(rowId)
      return next
    })
  const allSelected = rows.length > 0 && liveSelected.length === rows.length
  const editingRow = active && editing ? rows[rowIndex.get(active.row) ?? -1] : undefined
  const editingProperty = active && editing ? byId.get(active.col) : undefined

  return (
    <div className="relative w-max min-w-full pb-2">
      {liveSelected.length > 0 ? (
        <div
          role="toolbar"
          aria-label="Selected rows"
          className="absolute bottom-full left-0 z-20 mb-1.5 flex h-8 items-center gap-1 rounded-lg border border-border bg-background px-2 text-sm shadow-sm"
        >
          <span className="px-1 text-primary tabular-nums">{liveSelected.length} selected</span>
          {canEditValues ? (
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="text-destructive hover:text-destructive"
              disabled={trashRows.isPending}
              onClick={() => {
                trashRows.trash(liveSelected)
                setSelected(new Set())
              }}
            >
              Delete
            </Button>
          ) : null}
          <Button type="button" variant="ghost" size="xs" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
        </div>
      ) : null}
      <div
        ref={gridRef}
        role="grid"
        aria-label={database.database.title || 'Database'}
        aria-rowcount={rows.length + 1}
        aria-colcount={columns.length}
        aria-multiselectable="true"
        className="text-sm"
        style={{ width }}
        onKeyDown={onGridKeyDown}
        onPaste={onGridPaste}
      >
        <div role="rowgroup" className="sticky top-0 z-10 bg-background">
          <div role="row" aria-rowindex={1} className="group/header relative flex h-[33px] border-y border-border">
            <div className="absolute top-0 right-full flex h-full items-center pr-1.5">
              {rows.length > 0 ? (
                <Checkbox
                  aria-label="Select all rows"
                  checked={allSelected}
                  indeterminate={liveSelected.length > 0 && !allSelected}
                  className={cn(liveSelected.length === 0 && 'hover-fine:opacity-0 hover-fine:group-hover/header:opacity-100')}
                  onCheckedChange={() => setSelected(allSelected ? new Set() : new Set(rows.map((row) => row.id)))}
                />
              ) : null}
            </div>
            {columns.map((column, index) => {
              const { property } = column
              const sort = config.sorts.find((candidate) => candidate.property_id === property.id)
              return (
                <div
                  key={property.id}
                  role="columnheader"
                  aria-colindex={index + 1}
                  aria-sort={sort ? (sort.direction === 'asc' ? 'ascending' : 'descending') : undefined}
                  data-drop={columnDrop?.id === property.id ? columnDrop.side : undefined}
                  className="relative flex shrink-0 items-stretch border-r border-border data-[drop=left]:shadow-[inset_2px_0_0_var(--primary)] data-[drop=right]:shadow-[inset_-2px_0_0_var(--primary)]"
                  style={{ width: column.width }}
                  draggable={resizing === null}
                  onDragStart={(event) => {
                    if (resizeHeld.current) return event.preventDefault()
                    dragging.current = { kind: 'column', id: property.id }
                    event.dataTransfer.effectAllowed = 'move'
                    event.dataTransfer.setData('text/plain', property.name)
                  }}
                  onDragEnd={() => {
                    dragging.current = null
                    setColumnDrop(null)
                  }}
                  onDragOver={(event) => {
                    if (dragging.current?.kind !== 'column') return
                    event.preventDefault()
                    const box = event.currentTarget.getBoundingClientRect()
                    const side = event.clientX < box.left + box.width / 2 ? 'left' : 'right'
                    if (columnDrop?.id !== property.id || columnDrop.side !== side) setColumnDrop({ id: property.id, side })
                  }}
                  onDrop={dropColumn}
                >
                  <ColumnHeaderMenu property={property} config={config} onConfig={onConfig} onFilter={onFilter} />
                  <div
                    role="separator"
                    aria-orientation="vertical"
                    aria-label={`Resize ${property.name}`}
                    className="absolute top-0 -right-[3px] z-10 h-full w-[5px] cursor-col-resize hover:bg-primary/50 data-[active=true]:bg-primary"
                    data-active={resizing?.id === property.id}
                    onPointerDown={(event) => startResize(event, column)}
                  />
                </div>
              )
            })}
            <div className="flex shrink-0 items-center" style={{ width: ADD_COLUMN_WIDTH }}>
              {canEditSchema ? <AddPropertyButton /> : null}
            </div>
          </div>
        </div>
        <div ref={bodyRef} role="rowgroup" className="relative" style={{ height: virtualizer.getTotalSize() }}>
          {virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index]
            return (
              <div
                key={item.key}
                ref={virtualizer.measureElement}
                data-index={item.index}
                role="row"
                aria-rowindex={item.index + 2}
                aria-selected={selected.has(row.id)}
                data-drop={rowDrop?.id === row.id ? rowDrop.side : undefined}
                className="group/row absolute top-0 left-0 flex w-full border-b border-border data-[drop=after]:shadow-[inset_0_-2px_0_var(--primary)] data-[drop=before]:shadow-[inset_0_2px_0_var(--primary)] aria-selected:bg-primary/5"
                style={{ transform: `translateY(${item.start - scrollMargin}px)`, minHeight: ROW_HEIGHT }}
                onDragOver={(event) => {
                  if (dragging.current?.kind !== 'row') return
                  event.preventDefault()
                  const box = event.currentTarget.getBoundingClientRect()
                  const side = event.clientY < box.top + box.height / 2 ? 'before' : 'after'
                  if (rowDrop?.id !== row.id || rowDrop.side !== side) setRowDrop({ id: row.id, side })
                }}
                onDrop={dropRow}
              >
                <div className="absolute top-0 right-full flex h-[33px] items-center gap-0.5 pr-1.5">
                  {canEditValues ? (
                    <Tip label={sorted ? 'Remove sorts to reorder rows' : 'Drag to move'}>
                      <span
                        role="button"
                        tabIndex={-1}
                        aria-label="Drag to move"
                        aria-disabled={sorted}
                        draggable={!sorted}
                        className="flex h-6 w-4 cursor-grab items-center justify-center rounded-sm text-muted-foreground hover:bg-muted aria-disabled:cursor-default hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100"
                        onDragStart={(event) => {
                          dragging.current = { kind: 'row', id: row.id }
                          event.dataTransfer.effectAllowed = 'move'
                          event.dataTransfer.setData('text/plain', row.title)
                        }}
                        onDragEnd={() => {
                          dragging.current = null
                          setRowDrop(null)
                        }}
                      >
                        <GripIcon />
                      </span>
                    </Tip>
                  ) : null}
                  <Checkbox
                    aria-label={`Select ${row.title || 'Untitled'}`}
                    checked={selected.has(row.id)}
                    className={cn(liveSelected.length === 0 && 'hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100')}
                    onCheckedChange={() => toggleSelected(row.id)}
                  />
                </div>
                {columns.map((column, index) => {
                  const { property } = column
                  const isActive = active?.row === row.id && active.col === property.id
                  const isEditing = isActive && editing
                  const inline = property.type === 'title' || property.type === 'number' || property.type === 'url'
                  return (
                    <div
                      key={property.id}
                      role="gridcell"
                      aria-colindex={index + 1}
                      aria-readonly={!editable(property) || undefined}
                      data-cell={`${row.id}:${property.id}`}
                      data-active={isActive || undefined}
                      data-editing={isEditing || undefined}
                      tabIndex={isActive || (!active && item.index === 0 && index === 0) ? 0 : -1}
                      className={cn(
                        'group/cell relative flex shrink-0 cursor-default items-center border-r border-border px-2 outline-none',
                        config.wrap_cells ? 'min-h-[32px] items-start py-[6px]' : 'h-[32px] overflow-hidden',
                        'data-active:shadow-[inset_0_0_0_2px_color-mix(in_oklch,var(--primary)_60%,transparent)] data-editing:bg-background',
                      )}
                      style={{ width: column.width }}
                      onFocus={() => {
                        if (!isActive) setActive({ row: row.id, col: property.id })
                      }}
                      onClick={(event) => {
                        if (isEditing) return
                        if ((event.target as HTMLElement).closest('a, [data-slot=checkbox], [data-open-row]')) return
                        // A click on the title opens the row in the peek, where the title is edited (Enter still edits here).
                        if (property.type === 'title') {
                          setActive({ row: row.id, col: property.id })
                          return onOpenRow(row.id)
                        }
                        activate(row, property, event.currentTarget)
                      }}
                    >
                      {isEditing && inline ? null : (
                        <CellView
                          property={property}
                          row={row}
                          width={column.width}
                          wrap={config.wrap_cells}
                          onToggle={editable(property) && property.type === 'checkbox' ? () => toggleCheckbox(row, property) : undefined}
                        />
                      )}
                      {isEditing && editingRow && editingProperty ? (
                        <CellEditor
                          property={editingProperty}
                          row={editingRow}
                          anchor={editAnchor}
                          onCommit={(value) => commit(row, property, value)}
                          onTitle={(title) => commitTitle(row, title)}
                          onOptions={canEditSchema ? saveOptions(property) : undefined}
                          onClose={endEdit}
                        />
                      ) : null}
                      {property.type === 'title' && !isEditing ? (
                        <Button
                          type="button"
                          variant="outline"
                          size="xs"
                          data-open-row=""
                          className="absolute top-1/2 right-1.5 h-6 -translate-y-1/2 gap-1 bg-background px-1.5 text-xs text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100"
                          onClick={() => onOpenRow(row.id)}
                        >
                          <ArrowUpRight className="size-3" />
                          Open
                        </Button>
                      ) : null}
                    </div>
                  )
                })}
              </div>
            )
          })}
        </div>
        {rows.length === 0 ? (
          <div className="flex h-24 flex-col items-center justify-center gap-2 border-b border-border text-sm text-muted-foreground" style={{ width: width - ADD_COLUMN_WIDTH }}>
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
            className="flex h-[33px] w-full items-center gap-1.5 border-b border-border px-2 text-sm text-muted-foreground outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
            onClick={onAddRow}
          >
            <Add className="size-4" />
            New
          </button>
        ) : null}
      </div>
    </div>
  )
}
