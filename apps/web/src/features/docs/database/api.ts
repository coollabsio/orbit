import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import {
  createDatabaseLink,
  createDatabaseProperty,
  createDatabaseRow,
  createDatabaseView,
  deleteDatabaseProperty,
  deleteDatabaseView,
  getDatabase,
  moveDatabaseRow,
  restorePage,
  trashDatabaseRows,
  updateDatabaseProperty,
  updateDatabaseRowValues,
  updateDatabaseView,
  updatePage,
} from '@orbit/api/generated/sdk.gen'
import type {
  CreateDatabaseLinkBody,
  CreateDatabasePropertyBody,
  CreateDatabaseRowBody,
  CreateDatabaseViewBody,
  DatabaseDetail,
  DatabaseLinkedViews,
  DatabaseProperty,
  DatabasePropertyUpdate,
  DatabaseRelatedPage,
  DatabaseRow,
  DatabaseView,
  DatabaseViewConfig,
  UpdateDatabasePropertyBody,
  UpdateDatabaseViewBody,
} from '@orbit/api/generated/types.gen'
import { isPaymentRequired } from '@orbit/api/problem'
import { databaseErrorMessage } from '@orbit/database/errors'
import { parseDatabase, patchValues } from '@orbit/database/values'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/** The cache key of a database, or of a linked view block's copy (its views are the link's; under the database's key). */
function detailKey(workspaceId: string, databaseId: string, linkId?: string | null) {
  return linkId ? ([...queryKeys.database(workspaceId, databaseId), 'link', linkId] as const) : queryKeys.database(workspaceId, databaseId)
}

/** The viewer's IANA zone (`?tz=` of reads with computed values). */
export function viewerTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
}

/** A database with its own views, or with the views of the linked view block `linkId`. */
export function useDatabase(workspaceId: string, databaseId: string, linkId?: string | null) {
  return useQuery({
    queryKey: detailKey(workspaceId, databaseId, linkId),
    queryFn: async () => {
      const { data } = await getDatabase({
        client: apiClient,
        path: { workspace_id: workspaceId, database_id: databaseId },
        // Formulas compute `today()` and date parts in the viewer's zone.
        query: { tz: viewerTimeZone(), ...(linkId ? { link: linkId } : {}) },
        throwOnError: true,
      })
      return required(data, 'Database response was empty.')
    },
    // A stable function: TanStack Query parses once per fetched detail.
    select: parseDatabase,
  })
}

type Path = { workspace_id: string; database_id: string }
type Detail = DatabaseDetail
type Snapshot = { detail: Detail | undefined }

export interface DatabaseMutation<Vars, Result> {
  request: (path: Path, vars: Vars) => Promise<Result>
  /** The cache change shown at once; rolled back when the request fails. */
  optimistic?: (detail: Detail, vars: Vars) => Detail
  /** Puts the server's answer into the cache. */
  reconcile?: (detail: Detail, result: Result, vars: Vars) => Detail
  /** Refetch after success (positions renumbered or values converted on the server). */
  refetch?: boolean
  error: string
}

/**
 * One write to a database. The optimistic change is undone on error (then the query refetches, in case another write
 * landed in between) and a toast says what failed.
 */
export function useDatabaseMutation<Vars, Result>(workspaceId: string, databaseId: string, spec: DatabaseMutation<Vars, Result>, linkId?: string | null) {
  const queryClient = useQueryClient()
  const key = detailKey(workspaceId, databaseId, linkId)
  const path = { workspace_id: workspaceId, database_id: databaseId }
  return useMutation<Result, Error, Vars, Snapshot>({
    mutationFn: (vars) => spec.request(path, vars),
    onMutate: async (vars) => {
      await queryClient.cancelQueries({ queryKey: key })
      const detail = queryClient.getQueryData<Detail>(key)
      if (detail && spec.optimistic) queryClient.setQueryData<Detail>(key, spec.optimistic(detail, vars))
      return { detail }
    },
    onError: (error, _vars, snapshot) => {
      if (snapshot?.detail && spec.optimistic) queryClient.setQueryData(key, snapshot.detail)
      void queryClient.invalidateQueries({ queryKey: key })
      // A read-only workspace has its own banner and toast.
      if (!isPaymentRequired(error)) toast.error(databaseErrorMessage(error, spec.error))
    },
    onSuccess: (result, vars) => {
      if (spec.reconcile) {
        const detail = queryClient.getQueryData<Detail>(key)
        if (detail) queryClient.setQueryData<Detail>(key, spec.reconcile(detail, result, vars))
      }
      if (spec.refetch) void queryClient.invalidateQueries({ queryKey: key })
    },
  })
}

/** A row, or a template (its page is edited like a row), changed in the cached database. */
function withRow(detail: Detail, rowId: string, change: (row: DatabaseRow) => DatabaseRow): Detail {
  return {
    ...detail,
    rows: detail.rows.map((row) => (row.id === rowId ? change(row) : row)),
    ...(detail.templates ? { templates: detail.templates.map((template) => (template.id === rowId ? { ...change(template), name: template.name } : template)) } : {}),
  }
}

/** Patches a row in the cached database, e.g. the title typed on its page (no request; rows not loaded stay as they are). */
export function patchDatabaseRow(queryClient: QueryClient, workspaceId: string, databaseId: string, rowId: string, patch: Partial<DatabaseRow>): void {
  queryClient.setQueryData<Detail>(queryKeys.database(workspaceId, databaseId), (detail) => detail && withRow(detail, rowId, (row) => ({ ...row, ...patch })))
}

export type ValuesInput = { rowId: string; values: Record<string, unknown> }

/**
 * Adds pages a relation cell now links to (as its picker knows them) to the cached database and its linked view
 * copies: a write's response has no `related_pages`, the next read brings them.
 */
export function rememberRelatedPages(queryClient: QueryClient, workspaceId: string, databaseId: string, pages: Readonly<Record<string, DatabaseRelatedPage>>): void {
  queryClient.setQueriesData<Detail>({ queryKey: queryKeys.database(workspaceId, databaseId) }, (detail) =>
    detail?.database && Array.isArray(detail.rows) ? { ...detail, related_pages: { ...detail.related_pages, ...pages } } : detail,
  )
}

export function useUpdateValues(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<ValuesInput, DatabaseRow>(workspaceId, databaseId, {
    request: async (path, { rowId, values }) => {
      const { data } = await updateDatabaseRowValues({ client: apiClient, path: { ...path, row_id: rowId }, body: values, throwOnError: true })
      return required(data, 'Update values response was empty.')
    },
    optimistic: (detail, { rowId, values }) => withRow(detail, rowId, (row) => ({ ...row, values: patchValues(row.values, values) })),
    // A write's row has no gallery preview (only reads of a gallery send one): the cached one stays.
    reconcile: (detail, row) => withRow(detail, row.id, (cached) => (cached.preview ? { ...row, preview: cached.preview } : row)),
    error: 'Could not save the value.',
  })
}

export type TitleInput = { rowId: string; title: string; version: number }

/** A row title is its page title: `PATCH /pages/{row}` with the row's version. A 409 refetches the database. */
export function useUpdateRowTitle(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<TitleInput, { version: number; title: string; updated_at: string; updated_by: string }>(workspaceId, databaseId, {
    request: async (_path, { rowId, title, version }) => {
      const { data } = await updatePage({
        client: apiClient,
        path: { workspace_id: workspaceId, page_id: rowId },
        body: { title, expected_version: version },
        throwOnError: true,
      })
      return required(data, 'Update page response was empty.')
    },
    optimistic: (detail, { rowId, title }) => withRow(detail, rowId, (row) => ({ ...row, title })),
    reconcile: (detail, page, { rowId }) =>
      withRow(detail, rowId, (row) => ({ ...row, title: page.title, version: page.version, updated_at: page.updated_at, updated_by: page.updated_by })),
    error: 'Could not rename the row.',
  })
}

export function useCreateRow(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<CreateDatabaseRowBody, DatabaseRow>(workspaceId, databaseId, {
    request: async (path, body) => {
      const { data } = await createDatabaseRow({ client: apiClient, path, body, throwOnError: true })
      return required(data, 'Create row response was empty.')
    },
    reconcile: (detail, row) => ({
      ...detail,
      database: { ...detail.database, row_count: detail.database.row_count + 1 },
      rows: [...detail.rows.map((other) => (other.position >= row.position ? { ...other, position: other.position + 1 } : other)), row],
    }),
    error: 'Could not add the row.',
  })
}

export type MoveInput = { rowId: string; afterRowId: string | null; values?: Record<string, unknown> }

/** Manual order; `values` are written in the same request (a board card dropped into another column). */
export function useMoveRow(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<MoveInput, DatabaseRow>(workspaceId, databaseId, {
    request: async (path, { rowId, afterRowId, values }) => {
      const { data } = await moveDatabaseRow({
        client: apiClient,
        path: { ...path, row_id: rowId },
        body: { after_row_id: afterRowId, ...(values ? { values } : {}) },
        throwOnError: true,
      })
      return required(data, 'Move row response was empty.')
    },
    optimistic: (detail, { rowId, afterRowId, values }) => {
      const rows = moveRowAfter(detail.rows, rowId, afterRowId)
      return { ...detail, rows: values ? rows.map((row) => (row.id === rowId ? { ...row, values: patchValues(row.values, values) } : row)) : rows }
    },
    refetch: true,
    error: 'Could not move the row.',
  })
}

/** Rows renumbered 0..n with `rowId` right after `afterRowId` (`null`: first), as the server orders them. */
export function moveRowAfter(rows: DatabaseRow[], rowId: string, afterRowId: string | null): DatabaseRow[] {
  const ordered = [...rows].sort((a, b) => a.position - b.position || a.id.localeCompare(b.id))
  const moving = ordered.find((row) => row.id === rowId)
  if (!moving) return rows
  const rest = ordered.filter((row) => row.id !== rowId)
  const at = afterRowId === null ? 0 : rest.findIndex((row) => row.id === afterRowId) + 1
  rest.splice(at, 0, moving)
  return rest.map((row, position) => ({ ...row, position }))
}

/** Moves rows to the trash; the toast offers Undo, which restores each row page. */
export function useTrashRows(workspaceId: string, databaseId: string) {
  const queryClient = useQueryClient()
  const mutation = useDatabaseMutation<{ rows: DatabaseRow[] }, void>(workspaceId, databaseId, {
    request: async (path, { rows }) => {
      await trashDatabaseRows({ client: apiClient, path, body: { row_ids: rows.map((row) => row.id) }, throwOnError: true })
    },
    optimistic: (detail, { rows }) => {
      const gone = new Set(rows.map((row) => row.id))
      const kept = detail.rows.filter((row) => !gone.has(row.id))
      return { ...detail, database: { ...detail.database, row_count: kept.length }, rows: kept }
    },
    refetch: true,
    error: 'Could not delete the rows.',
  })
  const trash = (rows: DatabaseRow[]) =>
    mutation.mutate(
      { rows },
      {
        onSuccess: () =>
          toast.success(rows.length === 1 ? 'Row moved to the trash.' : `${rows.length} rows moved to the trash.`, {
            action: { label: 'Undo', onClick: () => void restoreRows(queryClient, workspaceId, databaseId, rows) },
          }),
      },
    )
  return { trash, isPending: mutation.isPending }
}

async function restoreRows(queryClient: QueryClient, workspaceId: string, databaseId: string, rows: DatabaseRow[]) {
  // Trashing bumps each row page's version once.
  const results = await Promise.allSettled(
    rows.map((row) =>
      restorePage({ client: apiClient, path: { workspace_id: workspaceId, page_id: row.id }, body: { expected_version: row.version + 1 }, throwOnError: true }),
    ),
  )
  const failed = results.find((result) => result.status === 'rejected')
  if (failed) toast.error(databaseErrorMessage(failed.reason, 'Could not restore every row. Find them in the trash.'))
  void queryClient.invalidateQueries({ queryKey: queryKeys.database(workspaceId, databaseId) })
  void queryClient.invalidateQueries({ queryKey: queryKeys.pages.trash(workspaceId) })
}

export function useCreateProperty(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<CreateDatabasePropertyBody, DatabaseProperty>(workspaceId, databaseId, {
    request: async (path, body) => {
      const { data } = await createDatabaseProperty({ client: apiClient, path, body, throwOnError: true })
      return required(data, 'Create property response was empty.')
    },
    // Known at once, so a view config can place its column (a copy goes right after its source, as on the server); the
    // refetch brings the renumbered positions and views.
    reconcile: (detail, property, { duplicate_of: source }) => ({
      ...detail,
      properties: [...detail.properties, property],
      views: source ? detail.views.map((view) => withCopiedColumn(view, source, property.id)) : detail.views,
    }),
    refetch: true,
    error: 'Could not add the property.',
  })
}

export type PropertyInput = { propertyId: string; body: UpdateDatabasePropertyBody }

export function useUpdateProperty(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<PropertyInput, unknown>(workspaceId, databaseId, {
    request: async (path, { propertyId, body }) => {
      const { data } = await updateDatabaseProperty({ client: apiClient, path: { ...path, property_id: propertyId }, body, throwOnError: true })
      return required(data, 'Update property response was empty.')
    },
    // A type change converts values on the server: it shows after the refetch. Names and configs show at once.
    optimistic: (detail, { propertyId, body }) => ({
      ...detail,
      properties: detail.properties.map((property) =>
        property.id !== propertyId
          ? property
          : { ...property, ...(body.name ? { name: body.name } : {}), ...(body.config && !body.type ? { config: body.config } : {}) },
      ),
    }),
    refetch: true,
    error: 'Could not change the property.',
  })
}

/** A view with the column of property `copyId` right after the column of `sourceId`, as wide and as hidden. */
function withCopiedColumn(view: DatabaseView, sourceId: string, copyId: string): DatabaseView {
  const columns = [...(view.config.columns ?? [])]
  const at = columns.findIndex((column) => column.property_id === sourceId)
  if (at === -1) return view
  columns.splice(at + 1, 0, { ...columns[at], property_id: copyId })
  return { ...view, config: { ...view.config, columns } }
}

/** What a property change would do (`converted`, `cleared`): the server runs it without saving it. */
export async function previewPropertyUpdate(workspaceId: string, databaseId: string, propertyId: string, body: UpdateDatabasePropertyBody) {
  const { data } = await updateDatabaseProperty({
    client: apiClient,
    path: { workspace_id: workspaceId, database_id: databaseId, property_id: propertyId },
    body: { ...body, dry_run: true },
    throwOnError: true,
  })
  return required<DatabasePropertyUpdate>(data, 'Update property response was empty.')
}

export function useDeleteProperty(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<string, void>(workspaceId, databaseId, {
    request: async (path, propertyId) => {
      await deleteDatabaseProperty({ client: apiClient, path: { ...path, property_id: propertyId }, throwOnError: true })
    },
    optimistic: (detail, propertyId) => ({ ...detail, properties: detail.properties.filter((property) => property.id !== propertyId) }),
    refetch: true,
    error: 'Could not delete the property.',
  })
}

/** View writes go to the database's own views, or to the views of the linked view block `linkId`. */
export function useCreateView(workspaceId: string, databaseId: string, linkId?: string | null) {
  return useDatabaseMutation<CreateDatabaseViewBody, DatabaseDetail['views'][number]>(
    workspaceId,
    databaseId,
    {
      request: async (path, body) => {
        const { data } = await createDatabaseView({ client: apiClient, path, body: linkId ? { ...body, link_id: linkId } : body, throwOnError: true })
        return required(data, 'Create view response was empty.')
      },
      reconcile: (detail, view) => ({ ...detail, views: [...detail.views, view] }),
      refetch: true,
      error: 'Could not add the view.',
    },
    linkId,
  )
}

export type ViewInput = { viewId: string; body: UpdateDatabaseViewBody }

export function useUpdateView(workspaceId: string, databaseId: string, linkId?: string | null) {
  return useDatabaseMutation<ViewInput, DatabaseDetail['views'][number]>(workspaceId, databaseId, {
    request: async (path, { viewId, body }) => {
      const { data } = await updateDatabaseView({
        client: apiClient,
        path: { ...path, view_id: viewId },
        ...(linkId ? { query: { link_id: linkId } } : {}),
        body,
        throwOnError: true,
      })
      return required(data, 'Update view response was empty.')
    },
    optimistic: (detail, { viewId, body }) => ({
      ...detail,
      views: detail.views.map((view) =>
        view.id !== viewId ? view : { ...view, ...(body.name ? { name: body.name } : {}), ...(body.config ? { config: body.config } : {}) },
      ),
    }),
    reconcile: (detail, saved) => ({ ...detail, views: detail.views.map((view) => (view.id === saved.id ? saved : view)) }),
    refetch: false,
    error: 'Could not save the view.',
  }, linkId)
}

export function useDeleteView(workspaceId: string, databaseId: string, linkId?: string | null) {
  return useDatabaseMutation<string, void>(workspaceId, databaseId, {
    request: async (path, viewId) => {
      await deleteDatabaseView({ client: apiClient, path: { ...path, view_id: viewId }, ...(linkId ? { query: { link_id: linkId } } : {}), throwOnError: true })
    },
    optimistic: (detail, viewId) => ({ ...detail, views: detail.views.filter((view) => view.id !== viewId) }),
    error: 'Could not delete the view.',
  }, linkId)
}

/** Links a database to a block of `host_page_id` (a linked view block) with one view of its own. */
export async function createLink(workspaceId: string, databaseId: string, body: CreateDatabaseLinkBody): Promise<DatabaseLinkedViews> {
  const { data } = await createDatabaseLink({ client: apiClient, path: { workspace_id: workspaceId, database_id: databaseId }, body, throwOnError: true })
  return required(data, 'Create link response was empty.')
}

export type { DatabaseViewConfig }
