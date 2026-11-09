import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import {
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
  CreateDatabasePropertyBody,
  CreateDatabaseRowBody,
  CreateDatabaseViewBody,
  DatabaseDetail,
  DatabaseProperty,
  DatabasePropertyUpdate,
  DatabaseRow,
  DatabaseView,
  DatabaseViewConfig,
  UpdateDatabasePropertyBody,
  UpdateDatabaseViewBody,
} from '@orbit/api/generated/types.gen'
import { ApiProblem, isPaymentRequired } from '@orbit/api/problem'
import { parseDatabase } from '@orbit/database/values'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function useDatabase(workspaceId: string, databaseId: string) {
  return useQuery({
    queryKey: queryKeys.database(workspaceId, databaseId),
    queryFn: async () => {
      const { data } = await getDatabase({ client: apiClient, path: { workspace_id: workspaceId, database_id: databaseId }, throwOnError: true })
      return required(data, 'Database response was empty.')
    },
    // A stable function: TanStack Query parses once per fetched detail.
    select: parseDatabase,
  })
}

/** The toast for a failed database write: the lock and the row cap have their own words. */
export function databaseErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'page_locked') return 'This database is locked. Unlock it to change its properties and views.'
    if (error.code === 'database_row_limit') return 'This database has the most rows it can hold (5,000).'
    if (error.code === 'database_last_view') return 'A database needs at least one view.'
    if (error.code === 'conflict') return 'This row changed somewhere else. The table now shows the latest version.'
    if (error.status === 422) return error.detail
  }
  return fallback
}

type Path = { workspace_id: string; database_id: string }
type Detail = DatabaseDetail
type Snapshot = { detail: Detail | undefined }

interface DatabaseMutation<Vars, Result> {
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
function useDatabaseMutation<Vars, Result>(workspaceId: string, databaseId: string, spec: DatabaseMutation<Vars, Result>) {
  const queryClient = useQueryClient()
  const key = queryKeys.database(workspaceId, databaseId)
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

function withRow(detail: Detail, rowId: string, change: (row: DatabaseRow) => DatabaseRow): Detail {
  return { ...detail, rows: detail.rows.map((row) => (row.id === rowId ? change(row) : row)) }
}

/** Stored values with a patch applied: `null` removes a value, as the server does. */
export function patchValues(values: DatabaseRow['values'], patch: Record<string, unknown>): DatabaseRow['values'] {
  const next = { ...values }
  for (const [propertyId, value] of Object.entries(patch)) {
    if (value === null) delete next[propertyId]
    else next[propertyId] = value
  }
  return next
}

export type ValuesInput = { rowId: string; values: Record<string, unknown> }

export function useUpdateValues(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<ValuesInput, DatabaseRow>(workspaceId, databaseId, {
    request: async (path, { rowId, values }) => {
      const { data } = await updateDatabaseRowValues({ client: apiClient, path: { ...path, row_id: rowId }, body: values, throwOnError: true })
      return required(data, 'Update values response was empty.')
    },
    optimistic: (detail, { rowId, values }) => withRow(detail, rowId, (row) => ({ ...row, values: patchValues(row.values, values) })),
    reconcile: (detail, row) => withRow(detail, row.id, () => row),
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

export function useMoveRow(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<{ rowId: string; afterRowId: string | null }, DatabaseRow>(workspaceId, databaseId, {
    request: async (path, { rowId, afterRowId }) => {
      const { data } = await moveDatabaseRow({ client: apiClient, path: { ...path, row_id: rowId }, body: { after_row_id: afterRowId }, throwOnError: true })
      return required(data, 'Move row response was empty.')
    },
    optimistic: (detail, { rowId, afterRowId }) => ({ ...detail, rows: moveRowAfter(detail.rows, rowId, afterRowId) }),
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
  if (results.some((result) => result.status === 'rejected')) toast.error('Could not restore every row. Find them in the trash.')
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

export function useCreateView(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<CreateDatabaseViewBody, DatabaseDetail['views'][number]>(workspaceId, databaseId, {
    request: async (path, body) => {
      const { data } = await createDatabaseView({ client: apiClient, path, body, throwOnError: true })
      return required(data, 'Create view response was empty.')
    },
    reconcile: (detail, view) => ({ ...detail, views: [...detail.views, view] }),
    refetch: true,
    error: 'Could not add the view.',
  })
}

export type ViewInput = { viewId: string; body: UpdateDatabaseViewBody }

export function useUpdateView(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<ViewInput, DatabaseDetail['views'][number]>(workspaceId, databaseId, {
    request: async (path, { viewId, body }) => {
      const { data } = await updateDatabaseView({ client: apiClient, path: { ...path, view_id: viewId }, body, throwOnError: true })
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
  })
}

export function useDeleteView(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<string, void>(workspaceId, databaseId, {
    request: async (path, viewId) => {
      await deleteDatabaseView({ client: apiClient, path: { ...path, view_id: viewId }, throwOnError: true })
    },
    optimistic: (detail, viewId) => ({ ...detail, views: detail.views.filter((view) => view.id !== viewId) }),
    error: 'Could not delete the view.',
  })
}

export type { DatabaseViewConfig }
