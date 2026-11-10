/** The database queries and writes, as on the web (`features/docs/database/api.ts`): optimistic, rolled back on error. */
import { isPaymentRequired } from '@orbit/api/problem'
import type { CreateDatabaseRowBody, DatabaseDetail, DatabaseRelatedPage, DatabaseRow } from '@orbit/api/generated/types.gen'
import { databaseErrorMessage } from '@orbit/database/errors'
import type { SelectOption, ViewConfig, ViewContext } from '@orbit/database/types'
import { parseDatabase, patchValues } from '@orbit/database/values'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'

import { billingKey, workspaceBilling } from '../data/billing'
import * as api from '../data/database-api'
import { keys } from '../data/keys'
import { useOrbit } from '../orbit-context'
import { useMembers } from '../queries'

/** How long a view change waits before it saves for everyone (spec D1). */
const SAVE_DELAY_MS = 500

/** Shows a failed write's message (the screen's notice line). */
export type Notify = (message: string) => void

/** `linkId` empty: the database's own views; else a linked view block's views (spec 3.4). */
export function useDatabase(databaseId: string, linkId = '') {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.database(workspaceId, databaseId, linkId), queryFn: () => api.database(client, workspaceId, databaseId, linkId), select: parseDatabase })
}

/** The viewer, the members and the clock that `@orbit/database` formats and filters with. */
export function useViewContext(): ViewContext {
  const { user } = useOrbit()
  const members = useMembers()
  const [now] = useState(() => Date.now())
  const [timeZone] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone)
  return useMemo(
    () => ({ userId: user.id, now, timeZone, members: new Map((members.data ?? []).map((member) => [member.user_id, member.display_name])) }),
    [members.data, now, timeZone, user.id],
  )
}

/**
 * Adds pages a relation cell of `rowId` now links to (as the picker knows them) to every cached database holding the
 * row: a write's response has no `related_pages`, the next read brings them.
 */
export function useRememberRelatedPages() {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return (rowId: string, pages: Readonly<Record<string, DatabaseRelatedPage>>) =>
    queryClient.setQueriesData<DatabaseDetail>({ queryKey: ['database', workspaceId] }, (detail) =>
      detail?.database && Array.isArray(detail.rows) && detail.rows.some((row) => row.id === rowId)
        ? { ...detail, related_pages: { ...detail.related_pages, ...pages } }
        : detail,
    )
}

/** A read-only workspace (Orbit Cloud billing) refuses every write; the billing line already says so. */
export function useWorkspaceReadOnly(): boolean {
  const { client, workspaceId } = useOrbit()
  const billing = useQuery({ queryKey: billingKey(workspaceId), queryFn: () => workspaceBilling(client, workspaceId), enabled: false })
  return billing.data?.access === 'read_only'
}

interface WriteSpec<Vars, Result> {
  request: (vars: Vars) => Promise<Result>
  optimistic?: (detail: DatabaseDetail, vars: Vars) => DatabaseDetail
  reconcile?: (detail: DatabaseDetail, result: Result, vars: Vars) => DatabaseDetail
  refetch?: boolean
  error: string
}

export function useDatabaseWrite<Vars, Result>(databaseId: string, linkId: string, notify: Notify, spec: WriteSpec<Vars, Result>) {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const key = keys.database(workspaceId, databaseId, linkId)
  return useMutation<Result, Error, Vars, { detail: DatabaseDetail | undefined }>({
    mutationFn: spec.request,
    onMutate: async (vars) => {
      await queryClient.cancelQueries({ queryKey: key })
      const detail = queryClient.getQueryData<DatabaseDetail>(key)
      if (detail && spec.optimistic) queryClient.setQueryData<DatabaseDetail>(key, spec.optimistic(detail, vars))
      return { detail }
    },
    onError: (error, _vars, snapshot) => {
      if (snapshot?.detail && spec.optimistic) queryClient.setQueryData(key, snapshot.detail)
      void queryClient.invalidateQueries({ queryKey: key })
      if (!isPaymentRequired(error)) notify(databaseErrorMessage(error, spec.error))
    },
    onSuccess: (result, vars) => {
      const detail = queryClient.getQueryData<DatabaseDetail>(key)
      if (detail && spec.reconcile) queryClient.setQueryData<DatabaseDetail>(key, spec.reconcile(detail, result, vars))
      if (spec.refetch) void queryClient.invalidateQueries({ queryKey: key })
    },
  })
}

function withRow(detail: DatabaseDetail, rowId: string, change: (row: DatabaseRow) => DatabaseRow): DatabaseDetail {
  return { ...detail, rows: detail.rows.map((row) => (row.id === rowId ? change(row) : row)) }
}

export function useUpdateValues(databaseId: string, notify: Notify, linkId = '') {
  const { client, workspaceId } = useOrbit()
  return useDatabaseWrite<{ rowId: string; values: Record<string, unknown> }, DatabaseRow>(databaseId, linkId, notify, {
    request: ({ rowId, values }) => api.saveValues(client, workspaceId, databaseId, rowId, values),
    optimistic: (detail, { rowId, values }) => withRow(detail, rowId, (row) => ({ ...row, values: patchValues(row.values, values) })),
    reconcile: (detail, row) => withRow(detail, row.id, () => row),
    error: 'Could not save the value.',
  })
}

export function useCreateRow(databaseId: string, notify: Notify, linkId = '') {
  const { client, workspaceId } = useOrbit()
  return useDatabaseWrite<CreateDatabaseRowBody, DatabaseRow>(databaseId, linkId, notify, {
    request: (body) => api.addRow(client, workspaceId, databaseId, body),
    reconcile: (detail, row) => ({
      ...detail,
      database: { ...detail.database, row_count: detail.database.row_count + 1 },
      rows: [...detail.rows.map((other) => (other.position >= row.position ? { ...other, position: other.position + 1 } : other)), row],
    }),
    error: 'Could not add the row.',
  })
}

function useUpdateView(databaseId: string, linkId: string, notify: Notify) {
  const { client, workspaceId } = useOrbit()
  return useDatabaseWrite<{ viewId: string; config: ViewConfig }, DatabaseDetail['views'][number]>(databaseId, linkId, notify, {
    request: ({ viewId, config }) => api.saveView(client, workspaceId, databaseId, linkId, viewId, config),
    optimistic: (detail, { viewId, config }) => ({ ...detail, views: detail.views.map((view) => (view.id === viewId ? { ...view, config } : view)) }),
    reconcile: (detail, saved) => ({ ...detail, views: detail.views.map((view) => (view.id === saved.id ? saved : view)) }),
    error: 'Could not save the view.',
  })
}

/** A select, multi-select or status property's options (a created option); the refetch brings the saved config. */
export function useSaveOptions(databaseId: string, notify: Notify) {
  const { client, workspaceId } = useOrbit()
  return useDatabaseWrite<{ propertyId: string; config: Record<string, unknown>; options: SelectOption[] }, void>(databaseId, '', notify, {
    request: ({ propertyId, config, options }) => api.saveOptions(client, workspaceId, databaseId, propertyId, config, options),
    refetch: true,
    error: 'Could not change the property.',
  })
}

/**
 * The view config drafts of one database (spec D1). A viewer who may change views saves each change for everyone
 * after 500 ms (leaving the screen saves at once); anyone else keeps it on this device until Reset.
 */
export function useViewDrafts(databaseId: string, linkId: string, canSave: boolean, notify: Notify) {
  const updateView = useUpdateView(databaseId, linkId, notify)
  const [drafts, setDrafts] = useState<Readonly<Record<string, ViewConfig>>>({})
  const pending = useRef(new Map<string, { config: ViewConfig; timer: ReturnType<typeof setTimeout> }>())
  const save = (viewId: string) => {
    const entry = pending.current.get(viewId)
    if (!entry) return
    clearTimeout(entry.timer)
    pending.current.delete(viewId)
    updateView.mutate({ viewId, config: entry.config }, { onSettled: () => setDrafts((current) => (current[viewId] === entry.config ? omit(current, viewId) : current)) })
  }
  const saveRef = useRef(save)
  useEffect(() => {
    saveRef.current = save
  })
  useEffect(() => {
    const queued = pending.current
    return () => {
      for (const viewId of [...queued.keys()]) saveRef.current(viewId)
    }
  }, [])
  return {
    drafts,
    change: (viewId: string, config: ViewConfig) => {
      setDrafts((current) => ({ ...current, [viewId]: config }))
      if (!canSave) return
      const queued = pending.current.get(viewId)
      if (queued) clearTimeout(queued.timer)
      pending.current.set(viewId, { config, timer: setTimeout(() => saveRef.current(viewId), SAVE_DELAY_MS) })
    },
    reset: (viewId: string) => setDrafts((current) => omit(current, viewId)),
  }
}

function omit<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  const { [key]: _removed, ...rest } = record
  return rest
}
