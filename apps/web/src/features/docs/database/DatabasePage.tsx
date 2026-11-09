import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { applyView, repairConfig } from '@orbit/database/view'
import { newRowValues } from '@orbit/database/newRow'
import type { Property, ViewConfig, ViewContext } from '@orbit/database/types'
import { useCurrentUser } from '@/features/auth/api'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspaceReadOnly } from '@/features/workspaces/billing'
import { useCreateRow, useDatabase, useUpdateView } from './api'
import { DatabaseEnvContext, type DatabaseEnv } from './context'
import { DatabaseBoard, type NewCard } from './DatabaseBoard'
import { DatabaseTable } from './DatabaseTable'
import { newRule, withPinnedRows, type Pin } from './databaseLib'
import { FilterBar } from './FilterBar'
import { ViewTabs } from './ViewTabs'
import { ViewToolbar } from './ViewToolbar'

const SAVE_DELAY_MS = 500

interface DatabasePageProps {
  workspaceId: string
  databaseId: string
  /** The page lock: properties and views are read-only, rows and values stay writable. */
  locked: boolean
  /** The page cover, full width above the title column. */
  cover: ReactNode
  /** The icon and title controls of the page. */
  title: ReactNode
}

const viewStorageKey = (databaseId: string) => `orbit:database-view:${databaseId}`

function readStoredView(databaseId: string): string | null {
  try {
    return localStorage.getItem(viewStorageKey(databaseId))
  } catch {
    return null
  }
}

/**
 * A database page (spec 7.1): the page header, view tabs, toolbar, filter bar and the table or board. It owns the view state:
 * the selected view, view config drafts (saved for everyone after 500 ms, or kept local for viewers who cannot change
 * the view), the search, and the rows added since the view last changed.
 */
export function DatabasePage({ workspaceId, databaseId, locked, cover, title }: DatabasePageProps) {
  const navigate = useNavigate()
  const query = useDatabase(workspaceId, databaseId)
  const membersQuery = useMembers(workspaceId)
  const me = useCurrentUser()
  const workspaceReadOnly = useWorkspaceReadOnly()
  const updateView = useUpdateView(workspaceId, databaseId)
  const createRow = useCreateRow(workspaceId, databaseId)
  const [searchParams, setSearchParams] = useSearchParams()
  const [now] = useState(() => Date.now())
  const [timeZone] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone)
  const [drafts, setDrafts] = useState<Readonly<Record<string, ViewConfig>>>({})
  const [search, setSearch] = useState('')
  const [pins, setPins] = useState<Pin[]>([])
  const [openRule, setOpenRule] = useState<number | null>(null)
  const [editRowId, setEditRowId] = useState<string | null>(null)
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  const [viewport, setViewport] = useState(0)

  const members = membersQuery.data ?? []
  const ctx: ViewContext = {
    userId: me.data?.id ?? '',
    now,
    timeZone,
    members: new Map(members.map((user) => [user.id, user.name])),
  }
  const canEditSchema = !locked && !workspaceReadOnly
  const env: DatabaseEnv = {
    workspaceId,
    databaseId,
    ctx,
    users: new Map(members.map((user) => [user.id, user])),
    canEditValues: !workspaceReadOnly,
    canEditSchema,
  }

  // The header column stays put while the table scrolls sideways: it is as wide as the scroller's content box.
  // `left-0` keeps it at the content edge: a sticky offset already starts inside the scroller's padding.
  useLayoutEffect(() => {
    if (!scroller) return
    const measure = () => {
      const style = getComputedStyle(scroller)
      setViewport(scroller.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(scroller)
    return () => observer.disconnect()
  }, [scroller])

  // View configs wait 500 ms, then save for everyone (spec D1). Leaving the page saves at once.
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

  const database = query.data
  if (!database) {
    return (
      <DatabaseScroller setScroller={setScroller}>
        <div className="sticky left-0" style={{ width: viewport || undefined }}>
          {cover}
          <div className="px-[30px]">
            {title}
            {query.isError ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                The database could not load.
                <Button type="button" variant="outline" size="sm" onClick={() => void query.refetch()}>
                  Retry
                </Button>
              </div>
            ) : (
              <Spinner className="text-muted-foreground" />
            )}
          </div>
        </div>
      </DatabaseScroller>
    )
  }

  const views = [...database.views].sort((a, b) => a.position - b.position)
  const requested = searchParams.get('view') ?? readStoredView(databaseId)
  const view = views.find((candidate) => candidate.id === requested) ?? views[0]
  if (!view) return null
  const draft = drafts[view.id]
  const config = repairConfig({ ...view, config: draft ?? view.config }, database.properties)
  const shown = applyView(database, { ...view, config }, search, ctx)
  const rows = withPinnedRows(shown, pins, database.rows)

  const selectView = (viewId: string) => {
    try {
      localStorage.setItem(viewStorageKey(databaseId), viewId)
    } catch {
      // private mode: the URL still holds the view
    }
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current)
        next.set('view', viewId)
        return next
      },
      { replace: true },
    )
    setPins([])
    setOpenRule(null)
  }
  const changeConfig = (next: ViewConfig) => {
    setDrafts((current) => ({ ...current, [view.id]: next }))
    setPins([])
    if (!canEditSchema) return
    const queued = pending.current.get(view.id)
    if (queued) clearTimeout(queued.timer)
    const viewId = view.id
    pending.current.set(viewId, { config: next, timer: setTimeout(() => saveRef.current(viewId), SAVE_DELAY_MS) })
  }
  const addFilter = (property: Property) => {
    changeConfig({ ...config, filter: { rules: [...config.filter.rules, newRule(property)] } })
    setOpenRule(config.filter.rules.length)
  }
  /** A new row with the values the filter needs; a board card also gets its column's value and place. */
  const addRow = (at: Pin['at'], card?: Omit<NewCard, 'at'>) => {
    const fill = newRowValues(config.filter.rules, database.properties, ctx)
    const after = card && 'after' in card ? card.after : at === 'top' ? null : undefined
    createRow.mutate(
      { ...(fill.title ? { title: fill.title } : {}), values: { ...fill.values, ...card?.values }, ...(after !== undefined ? { after_row_id: after } : {}) },
      {
        onSuccess: (row) => {
          setPins((current) => (at === 'top' ? [{ id: row.id, at }, ...current] : [...current, { id: row.id, at }]))
          setEditRowId(row.id)
        },
      },
    )
  }
  return (
    <DatabaseEnvContext.Provider value={env}>
      <DatabaseScroller setScroller={setScroller}>
        <div className="sticky left-0" style={{ width: viewport || undefined }}>
          {cover}
          <div className="px-[30px]">
            {title}
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
                onNew={() => addRow('top')}
              />
            </div>
            {draft && !canEditSchema ? (
              <div role="status" className="flex items-center gap-1.5 py-1.5 text-xs text-muted-foreground">
                Changes are only visible to you
                <span aria-hidden="true">·</span>
                <Button type="button" variant="link" size="xs" className="h-auto px-0 text-xs" onClick={() => setDrafts((current) => omit(current, view.id))}>
                  Reset
                </Button>
              </div>
            ) : null}
            <FilterBar properties={database.properties} config={config} onConfig={changeConfig} openRule={openRule} onOpenRule={setOpenRule} />
          </div>
        </div>
        {view.type === 'board' ? (
          <div className="px-[30px] pt-3">
            <DatabaseBoard
              key={view.id}
              database={database}
              config={config}
              rows={rows}
              onConfig={changeConfig}
              onAddCard={({ at, ...card }) => addRow(at, card)}
              onOpenRow={(rowId) => navigate(`/docs/${rowId}`)}
              filtered={config.filter.rules.length > 0 || search.trim() !== ''}
              onClearFilters={() => {
                setSearch('')
                changeConfig({ ...config, filter: { rules: [] } })
              }}
              editRowId={editRowId}
              onEditRowShown={() => setEditRowId(null)}
            />
          </div>
        ) : (
          <div className="px-[30px] pt-2">
            {/* The row drag handles and checkboxes (40 px) sit left of the table, in this padding plus the scroller's
                (54 px in all, 42 px below 900 px). */}
            <DatabaseTable
              key={view.id}
              database={database}
              config={config}
              rows={rows}
              scrollElement={scroller}
              onConfig={changeConfig}
              onFilter={(propertyId) => {
                const property = database.properties.find((candidate) => candidate.id === propertyId)
                if (property) addFilter(property)
              }}
              onAddRow={() => addRow('bottom')}
              onOpenRow={(rowId) => navigate(`/docs/${rowId}`)}
              filtered={config.filter.rules.length > 0 || search.trim() !== ''}
              onClearFilters={() => {
                setSearch('')
                changeConfig({ ...config, filter: { rules: [] } })
              }}
              editRowId={editRowId}
              onEditRowShown={() => setEditRowId(null)}
            />
          </div>
        )}
      </DatabaseScroller>
    </DatabaseEnvContext.Provider>
  )
}

/** The page's one scroller, both ways: the header sticks to its left edge, the table header to its top. */
function DatabaseScroller({ setScroller, children }: { setScroller: (element: HTMLDivElement | null) => void; children: ReactNode }) {
  return (
    <div
      ref={setScroller}
      data-print-root=""
      data-virtual-scroller=""
      className="min-h-0 flex-1 overflow-auto px-6 pt-8 pb-24 max-[899px]:px-3 max-[899px]:pt-[18px] max-[899px]:pb-14"
    >
      <div className="w-max min-w-full">{children}</div>
    </div>
  )
}

function omit<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  const { [key]: _removed, ...rest } = record
  return rest
}
