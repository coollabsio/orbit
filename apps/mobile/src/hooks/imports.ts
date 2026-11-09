/** The workspace imports' queries and writes, over `data/import-api.ts`. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiProblem } from '@orbit/api/problem'
import { useEffect, useMemo, useRef, useState } from 'react'

import * as api from '../data/import-api'
import { keys } from '../data/keys'
import type { PageSummary, Teamspace } from '../data/orbit'
import { useOrbit } from '../orbit-context'

/** A fresh import into the detail cache, and the list refreshed. */
export function useReconcileImport<K extends api.ImportKind>(kind: K) {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return (item: api.ImportRecord<K>) => {
    queryClient.setQueryData(api.importKeys.detail(workspaceId, kind, item.id), item)
    void queryClient.invalidateQueries({ queryKey: api.importKeys.list(workspaceId, kind), exact: true })
  }
}

/** What an import changes once it ends: the page tree, or the projects and tasks. */
function useRefreshImported(kind: api.ImportKind) {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return () => {
    if (api.docsImport(kind)) {
      void queryClient.invalidateQueries({ queryKey: keys.pages(workspaceId) })
    } else {
      void queryClient.invalidateQueries({ queryKey: keys.projects(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) })
    }
  }
}

/** The member's recent imports of this kind, polled slowly while one runs. */
export function useImports<K extends api.ImportKind>(kind: K) {
  const { client, workspaceId } = useOrbit()
  return useQuery({
    queryKey: api.importKeys.list(workspaceId, kind),
    queryFn: () => api.listImports(client, kind, workspaceId),
    refetchInterval: (query) => (query.state.data?.some((item) => api.isActiveImport(item.status)) ? api.IMPORT_POLL_MS * 2 : false),
  })
}

/**
 * One import without its tree, polled every 2 s while a job works on it. When it reaches a final state the imported
 * pages or projects and the imports list are refreshed.
 */
export function useImport<K extends api.ImportKind>(kind: K, importId: string | undefined) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = useRefreshImported(kind)
  const query = useQuery({
    queryKey: api.importKeys.detail(workspaceId, kind, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: () => api.getImport(client, kind, workspaceId, importId!),
    // a missing import stays missing: show "Import not found" at once
    retry: (count, error) => !api.isImportMissing(error) && count < 3,
    refetchInterval: (current) => (api.isActiveImport(current.state.data?.status) ? api.IMPORT_POLL_MS : false),
  })
  const status = query.data?.status
  const previous = useRef(status)
  useEffect(() => {
    const before = previous.current
    previous.current = status
    if (before !== undefined && before !== status && api.isFinalImport(status)) {
      refresh()
      void queryClient.invalidateQueries({ queryKey: api.importKeys.list(workspaceId, kind), exact: true })
    }
  }, [status])
  return query
}

/** The scan tree of a `ready` import, fetched once and never polled. */
export function useImportTree<K extends api.ImportKind>(kind: K, importId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({
    queryKey: api.importKeys.tree(workspaceId, kind, importId),
    staleTime: Infinity,
    queryFn: () => api.getImportTree(client, kind, workspaceId, importId),
  })
}

export function useStartImport<K extends api.ImportKind>(kind: K) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const reconcile = useReconcileImport(kind)
  return useMutation({
    mutationFn: ({ importId, body }: { importId: string; body: api.StartBody<K> }) => api.startImport(client, kind, workspaceId, importId, body),
    onSuccess: reconcile,
    onError: (error, { importId }) => {
      // expired or cancelled elsewhere: show the server's state
      if (error instanceof ApiProblem && error.status === 409) void queryClient.invalidateQueries({ queryKey: api.importKeys.detail(workspaceId, kind, importId), exact: true })
    },
  })
}

export function useCancelImport<K extends api.ImportKind>(kind: K) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const reconcile = useReconcileImport(kind)
  const refresh = useRefreshImported(kind)
  return useMutation({
    mutationFn: (importId: string) => api.cancelImport(client, kind, workspaceId, importId),
    onSuccess: (item) => {
      reconcile(item)
      // what was imported stays; the empty pages it created go to the trash
      refresh()
    },
    onError: (_error, importId) => void queryClient.invalidateQueries({ queryKey: api.importKeys.detail(workspaceId, kind, importId), exact: true }),
  })
}

/** Where a Docs import puts its top-level pages: a space (the default teamspace first), optionally inside a page. */
export function useImportDestination(teamspaces: Teamspace[] | undefined, pages: PageSummary[] | undefined) {
  const [chosenSpace, setChosenSpace] = useState<api.SpaceKey | null>(null)
  const [parentId, setParentId] = useState(api.TOP_LEVEL)
  const fallback = api.defaultTeamspace(teamspaces)
  const space: api.SpaceKey = chosenSpace ?? (fallback ? api.teamspaceSpace(fallback.id) : api.PRIVATE_SPACE)
  const parentOptions = useMemo(() => api.spacePages(pages ?? [], space), [pages, space])
  // a page that moved away (or was trashed) is no longer a valid choice
  const parent = parentId !== api.TOP_LEVEL && parentOptions.some(({ page }) => page.id === parentId) ? parentId : api.TOP_LEVEL
  const body: api.DestinationBody = parent !== api.TOP_LEVEL ? { parent_page_id: parent } : api.spaceBody(space)
  return {
    teamspaces,
    space,
    parent,
    parentOptions,
    body,
    chooseSpace: (next: api.SpaceKey) => {
      setChosenSpace(next)
      setParentId(api.TOP_LEVEL)
    },
    chooseParent: setParentId,
  }
}

export type ImportDestination = ReturnType<typeof useImportDestination>

/**
 * The connect step's send: the credentials go out once (never into query state), the new import lands in the cache,
 * and `onCreated` opens it. A refusal because an import already runs refreshes the list, so its link can show.
 */
export function useConnectImport<K extends api.ImportKind>(kind: K, onCreated: (item: api.ImportRecord<K>) => void) {
  const imports = useImports(kind)
  const reconcile = useReconcileImport(kind)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const running = imports.data?.find((item) => api.isActiveImport(item.status)) ?? null
  const submit = async (create: () => Promise<api.ImportRecord<K>>) => {
    if (pending) return
    setPending(true)
    setError(null)
    try {
      const created = await create()
      reconcile(created)
      onCreated(created)
    } catch (caught) {
      setError(caught)
      if (api.isImportInProgress(caught)) void imports.refetch()
    } finally {
      setPending(false)
    }
  }
  return { imports: imports.data, pending, error, setError, running, submit }
}
