import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { Button } from '@/components/ui/button'
import { applyView, repairConfig } from '@orbit/database/view'
import { newRule } from '@orbit/database/meta'
import { newRowValues } from '@orbit/database/newRow'
import type { Database, DatabaseView, Property, ViewConfig } from '@orbit/database/types'
import { useCreateRow, useUpdateView } from './api'
import { DatabaseEnvContext, type DatabaseEnv } from './context'
import { DatabaseBoard, type NewCard } from './DatabaseBoard'
import { DatabaseTable } from './DatabaseTable'
import { withPinnedRows, type Pin } from './databaseLib'
import { RowPeek } from './RowPeek'
import { FilterBar } from './FilterBar'
import { ViewTabs } from './ViewTabs'
import { ViewToolbar } from './ViewToolbar'

const SAVE_DELAY_MS = 500
/** The URL parameter that names the database block whose row the peek shows (blocks only; the full page has one). */
const PEEK_BLOCK_PARAM = 'row_block'

/** The parts a host lays out: the view tabs, toolbar and filter bar, then the table or board. */
export interface SurfaceParts {
  controls: ReactNode
  content: ReactNode
  board: boolean
}

interface DatabaseSurfaceProps {
  database: Database
  /** The views shown: the database's own, or a linked view block's. */
  views: DatabaseView[]
  env: DatabaseEnv
  /** The database page's lock (the toolbar's lock switch). */
  locked: boolean
  /** Where the selected view is remembered on this device. */
  viewKey: string
  /** The full page: the selected view is also in `?view=`, and the table is virtualized in `scrollElement`. */
  page?: { scrollElement: HTMLElement | null }
  /** A database block: its id (for the row peek in the URL) and the rows it shows before "Load more". */
  block?: { id: string; rowLimit: number }
  layout: (parts: SurfaceParts) => ReactNode
}

function readStoredView(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

/**
 * The view state of a database (spec 7.1), for its full page and for database blocks in pages: the selected view, view
 * config drafts (saved for everyone after 500 ms, or kept local for viewers who cannot change the view), the search, the
 * rows added since the view last changed and the row peek (`?row=`).
 */
export function DatabaseSurface({ database, views: unsorted, env, locked, viewKey, page, block, layout }: DatabaseSurfaceProps) {
  const { ctx, canEditViews, workspaceId, databaseId, linkId } = env
  const updateView = useUpdateView(workspaceId, databaseId, linkId)
  const createRow = useCreateRow(workspaceId, databaseId)
  const [searchParams, setSearchParams] = useSearchParams()
  const [drafts, setDrafts] = useState<Readonly<Record<string, ViewConfig>>>({})
  const [search, setSearch] = useState('')
  const [pins, setPins] = useState<Pin[]>([])
  const [openRule, setOpenRule] = useState<number | null>(null)
  const [editRowId, setEditRowId] = useState<string | null>(null)
  /** A row created from "New": the peek focuses its title. */
  const [focusRowId, setFocusRowId] = useState<string | null>(null)
  const [selectedView, setSelectedView] = useState(() => readStoredView(viewKey))
  const [rowLimit, setRowLimit] = useState(block?.rowLimit ?? Infinity)

  // View configs wait 500 ms, then save for everyone (spec D1). Leaving saves at once.
  const pending = useRef(new Map<string, { config: ViewConfig; timer: ReturnType<typeof setTimeout> }>())
  const saveView = (viewId: string) => {
    const entry = pending.current.get(viewId)
    if (!entry) return
    clearTimeout(entry.timer)
    pending.current.delete(viewId)
    updateView.mutate(
      { viewId, body: { config: entry.config } },
      { onSettled: () => setDrafts((current) => (current[viewId] === entry.config ? omit(current, viewId) : current)) },
    )
  }
  const saveRef = useRef(saveView)
  useEffect(() => {
    saveRef.current = saveView
  })
  useEffect(() => {
    const queued = pending.current
    return () => {
      for (const viewId of [...queued.keys()]) saveRef.current(viewId)
    }
  }, [])

  const peekParam = searchParams.get('row')
  const peekRowId = peekParam && (!block || searchParams.get(PEEK_BLOCK_PARAM) === block.id) ? peekParam : null
  /**
   * The side peek is in the URL (`?row=`, next to `?view=`): Back closes it and a link reopens it. Another row in an
   * open peek replaces the entry, so Back still closes the peek.
   */
  const setPeekRow = (rowId: string | null) => {
    if (rowId === peekRowId) return
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current)
        if (rowId) next.set('row', rowId)
        else next.delete('row')
        if (block && rowId) next.set(PEEK_BLOCK_PARAM, block.id)
        else next.delete(PEEK_BLOCK_PARAM)
        return next
      },
      { replace: rowId !== null && peekRowId !== null },
    )
  }

  const views = [...unsorted].sort((a, b) => a.position - b.position)
  const requested = (page ? searchParams.get('view') : null) ?? selectedView
  const view = views.find((candidate) => candidate.id === requested) ?? views[0]
  if (!view) return null
  const draft = drafts[view.id]
  const config = repairConfig({ ...view, config: draft ?? view.config }, database.properties)
  const shown = applyView(database, { ...view, config }, search, ctx)
  const board = view.type === 'board'
  // A block's table shows its first rows; a board limits each column itself.
  const rows = withPinnedRows(board ? shown : shown.slice(0, rowLimit), pins, database.rows)
  const hiddenRows = board ? 0 : Math.max(0, shown.length - rowLimit)

  const selectView = (viewId: string) => {
    try {
      localStorage.setItem(viewKey, viewId)
    } catch {
      // private mode: the URL (full page) or the state still holds the view
    }
    setSelectedView(viewId)
    if (page) {
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current)
          next.set('view', viewId)
          return next
        },
        { replace: true },
      )
    }
    setPins([])
    setOpenRule(null)
  }
  const changeConfig = (next: ViewConfig) => {
    setDrafts((current) => ({ ...current, [view.id]: next }))
    setPins([])
    if (!canEditViews) return
    const queued = pending.current.get(view.id)
    if (queued) clearTimeout(queued.timer)
    const viewId = view.id
    pending.current.set(viewId, { config: next, timer: setTimeout(() => saveRef.current(viewId), SAVE_DELAY_MS) })
  }
  const addFilter = (property: Property) => {
    changeConfig({ ...config, filter: { rules: [...config.filter.rules, newRule(property)] } })
    setOpenRule(config.filter.rules.length)
  }
  /**
   * A new row with the values the filter needs; a board card also gets its column's value and place. "New" in the
   * toolbar opens it in the peek; the others edit its title in place, so typing rows stays fast.
   */
  const addRow = (at: Pin['at'], card?: Omit<NewCard, 'at'>, open: 'inline' | 'peek' = 'inline') => {
    const fill = newRowValues(config.filter.rules, database.properties, ctx)
    const after = card && 'after' in card ? card.after : at === 'top' ? null : undefined
    createRow.mutate(
      { ...(fill.title ? { title: fill.title } : {}), values: { ...fill.values, ...card?.values }, ...(after !== undefined ? { after_row_id: after } : {}) },
      {
        onSuccess: (row) => {
          setPins((current) => (at === 'top' ? [{ id: row.id, at }, ...current] : [...current, { id: row.id, at }]))
          if (open === 'inline') return setEditRowId(row.id)
          setFocusRowId(row.id)
          setPeekRow(row.id)
        },
      },
    )
  }
  const filtered = config.filter.rules.length > 0 || search.trim() !== ''
  const clearFilters = () => {
    setSearch('')
    changeConfig({ ...config, filter: { rules: [] } })
  }

  const controls = (
    <>
      <div className="flex items-center gap-2 border-b border-border pb-1">
        <ViewTabs views={views} activeId={view.id} onSelect={selectView} />
        <span className="flex-1" />
        <ViewToolbar
          properties={database.properties}
          config={config}
          onConfig={changeConfig}
          onAddFilter={addFilter}
          search={search}
          onSearch={setSearch}
          locked={locked}
          viewId={view.id}
          viewType={view.type}
          onNew={() => addRow('top', undefined, 'peek')}
        />
      </div>
      {draft && !canEditViews ? (
        <div role="status" className="flex items-center gap-1.5 py-1.5 text-xs text-muted-foreground">
          Changes are only visible to you
          <span aria-hidden="true">·</span>
          <Button type="button" variant="link" size="xs" className="h-auto px-0 text-xs" onClick={() => setDrafts((current) => omit(current, view.id))}>
            Reset
          </Button>
        </div>
      ) : null}
      <FilterBar properties={database.properties} config={config} onConfig={changeConfig} openRule={openRule} onOpenRule={setOpenRule} />
    </>
  )
  const content = board ? (
    <DatabaseBoard
      key={view.id}
      database={database}
      config={config}
      rows={rows}
      onConfig={changeConfig}
      onAddCard={({ at, ...card }) => addRow(at, card)}
      onOpenRow={(rowId) => setPeekRow(rowId)}
      filtered={filtered}
      onClearFilters={clearFilters}
      editRowId={editRowId}
      onEditRowShown={() => setEditRowId(null)}
      pageSize={block?.rowLimit}
    />
  ) : (
    <>
      <DatabaseTable
        key={view.id}
        database={database}
        config={config}
        rows={rows}
        scrollElement={page?.scrollElement ?? null}
        virtualize={!!page}
        onConfig={changeConfig}
        onFilter={(propertyId) => {
          const property = database.properties.find((candidate) => candidate.id === propertyId)
          if (property) addFilter(property)
        }}
        onAddRow={() => addRow('bottom')}
        onOpenRow={(rowId) => setPeekRow(rowId)}
        filtered={filtered}
        onClearFilters={clearFilters}
        editRowId={editRowId}
        onEditRowShown={() => setEditRowId(null)}
      />
      {block && hiddenRows > 0 ? (
        <Button type="button" variant="ghost" size="sm" className="mt-1 text-muted-foreground" onClick={() => setRowLimit((limit) => limit + block.rowLimit)}>
          Load more
        </Button>
      ) : null}
    </>
  )

  return (
    <DatabaseEnvContext.Provider value={env}>
      {layout({ controls, content, board })}
      {peekRowId ? (
        <RowPeek
          workspaceId={workspaceId}
          databaseId={databaseId}
          rowId={peekRowId}
          viewId={view.id}
          order={rows.map((row) => row.id)}
          focusTitle={focusRowId === peekRowId}
          onShowRow={setPeekRow}
          onClose={() => setPeekRow(null)}
        />
      ) : null}
    </DatabaseEnvContext.Provider>
  )
}

function omit<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  const { [key]: _removed, ...rest } = record
  return rest
}
