/** The Docs writes, favorites, trash and comments, over `data/docs-api.ts`. The tree lists are in `src/queries.ts`. */
import type { DatabaseDetail, PageFavorite, PageKind } from '@orbit/api/generated/types.gen'
import { type QueryClient, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import type { UploadFile } from '../data/account-api'
import { type MoveDestination, moveRequest, paragraphBlocks, spaceRequest } from '../data/docs'
import * as docs from '../data/docs-api'
import { keys } from '../data/keys'
import * as api from '../data/orbit'
import type { Page, PageSummary, Teamspace } from '../data/orbit'
import { useOrbit } from '../orbit-context'
import { useMentionPeople, useProjects } from '../queries'

/**
 * A saved page into its detail cache and the tree, without a refetch. A database row is not in the tree: its title
 * and icon go to its database instead.
 */
function reconcile(queryClient: QueryClient, workspaceId: string, page: Page) {
  queryClient.setQueryData(keys.page(workspaceId, page.id), page)
  if (page.database_id) {
    // the prefix holds the database's own views and each linked view
    queryClient.setQueriesData<DatabaseDetail>({ queryKey: keys.database(workspaceId, page.database_id) }, (detail) =>
      detail && { ...detail, rows: detail.rows.map((row) => (row.id === page.id ? { ...row, title: page.title, icon: page.icon, version: page.version } : row)) },
    )
    return
  }
  const summary: PageSummary = {
    id: page.id,
    parent_id: page.parent_id,
    teamspace_id: page.teamspace_id,
    private: page.private,
    kind: page.kind,
    title: page.title,
    icon: page.icon,
    position: page.position,
    version: page.version,
    updated_at: page.updated_at,
  }
  queryClient.setQueryData<PageSummary[]>(keys.pages(workspaceId), (tree) =>
    tree ? (tree.some((item) => item.id === page.id) ? tree.map((item) => (item.id === page.id ? summary : item)) : [...tree, summary]) : tree,
  )
}

/** Creates a page in a space (`teamspace:<id>`, `private`, `null` for the default) or under a parent page. */
export function useCreatePage() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ space = null, parentId, kind = 'page' }: { space?: string | null; parentId?: string; kind?: PageKind }) =>
      docs.newPage(client, workspaceId, { ...(parentId ? { parent_id: parentId } : spaceRequest(space)), ...(kind === 'database' ? { kind } : {}) }),
    onSuccess: (page) => {
      reconcile(queryClient, workspaceId, page)
      void queryClient.invalidateQueries({ queryKey: keys.pages(workspaceId), exact: true })
    },
  })
}

export function useDuplicatePage() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ pageId, withChildren }: { pageId: string; withChildren: boolean }) => docs.duplicate(client, workspaceId, pageId, withChildren),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: keys.pages(workspaceId), exact: true }),
  })
}

/** Title, icon or cover. Reads the version from the cache, so quick edits in a row do not conflict. */
export function useSavePage(pageId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const current = () => queryClient.getQueryData<Page>(keys.page(workspaceId, pageId))
  return useMutation({
    mutationFn: (body: { title?: string; icon?: string | null; cover_url?: string | null }) => {
      const page = current()
      if (!page) throw new Error('The page is not loaded yet.')
      return docs.savePage(client, workspaceId, page, body)
    },
    onSuccess: (page) => reconcile(queryClient, workspaceId, page),
  })
}

/** Stores an image on the page (a cover, or an image block); resolves to its URL. */
export function useUploadPageImage(pageId: string) {
  const { client, workspaceId } = useOrbit()
  return useMutation({ mutationFn: (file: UploadFile) => docs.uploadPageImage(client, workspaceId, pageId, file) })
}

export function useFavorites() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: docs.docsKeys.favorites(workspaceId), queryFn: () => docs.favorites(client, workspaceId) })
}

/** True when `pageId` is one of `favorites`. */
export const isFavorite = (favorites: readonly PageFavorite[] | undefined, pageId: string) => favorites?.some((favorite) => favorite.page_id === pageId) ?? false

/**
 * Adds (appended last, shown with `page` at once) or removes a favorite; the star moves at once and goes back on an
 * error. A row shows its database's name from the tree at once.
 */
export function useToggleFavorite() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const key = docs.docsKeys.favorites(workspaceId)
  return useMutation({
    mutationFn: ({ pageId, favorite }: { pageId: string; favorite: boolean; page?: Pick<PageSummary, 'title' | 'icon' | 'kind'> & { database_id?: string | null } }) =>
      docs.setFavorite(client, workspaceId, pageId, favorite),
    onMutate: async ({ pageId, favorite, page }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const favorites = queryClient.getQueryData<PageFavorite[]>(key)
      const without = (favorites ?? []).filter((item) => item.page_id !== pageId)
      const databaseId = page?.database_id ?? null
      const database = databaseId ? queryClient.getQueryData<PageSummary[]>(keys.pages(workspaceId))?.find((item) => item.id === databaseId) : undefined
      const added: PageFavorite = {
        page_id: pageId,
        position: without.length,
        title: page?.title ?? '',
        icon: page?.icon ?? null,
        kind: page?.kind ?? 'page',
        database_id: databaseId,
        database_title: database ? database.title : null,
      }
      queryClient.setQueryData<PageFavorite[]>(key, favorite ? [...without, added] : without)
      return { favorites }
    },
    onError: (_error, _input, context) => queryClient.setQueryData(key, context?.favorites),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** Everything a trash change touches: the tree, the trash, the open pages and the databases (a row is a page). */
function useRefreshDocs() {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: keys.pages(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: ['page', workspaceId] })
    void queryClient.invalidateQueries({ queryKey: ['database', workspaceId] })
  }
}

/**
 * "Move to" for one page, in two steps so the screen can confirm in between: `prepare` reads the page's current
 * version and, for a row that leaves its database, asks the server how many values the move deletes (a dry run);
 * `move` then moves it. The server refuses a database into a database and a template.
 */
export function useMovePageTo() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = useRefreshDocs()
  const move = useMutation({
    mutationFn: ({ page, request }: { page: Page; request: ReturnType<typeof moveRequest> }) => docs.movePageTo(client, workspaceId, page, request),
    onSettled: refresh,
  })
  return {
    prepare: async (pageId: string, destination: MoveDestination): Promise<{ dropped: number; move: () => Promise<unknown> }> => {
      const page = await api.page(client, workspaceId, pageId)
      const request = moveRequest(queryClient.getQueryData<PageSummary[]>(keys.pages(workspaceId)) ?? [], pageId, destination)
      const leaves = page.database_id !== null && request.parent_id !== page.database_id
      const dropped = leaves ? await docs.movePageTo(client, workspaceId, page, request, true) : 0
      return { dropped, move: () => move.mutateAsync({ page, request }) }
    },
  }
}

/** Server search a moment after the last key (idle while blank), keeping the last results while the next ones load. */
export function usePageSearch(query: string) {
  const { client, workspaceId } = useOrbit()
  const typed = query.trim().slice(0, 200)
  const [q, setQ] = useState(typed)
  useEffect(() => {
    const timer = setTimeout(() => setQ(typed), 250)
    return () => clearTimeout(timer)
  }, [typed])
  return useQuery({
    queryKey: docs.docsKeys.search(workspaceId, q),
    queryFn: () => docs.search(client, workspaceId, q),
    enabled: q.length > 0,
    placeholderData: (previous) => previous,
  })
}

export function useRecentPages() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: docs.docsKeys.recent(workspaceId), queryFn: () => docs.recentPages(client, workspaceId) })
}

export function useTrashPage() {
  const { client, workspaceId } = useOrbit()
  const refresh = useRefreshDocs()
  return useMutation({ mutationFn: (page: Pick<PageSummary, 'id' | 'version'>) => docs.trashPage(client, workspaceId, page), onSettled: refresh })
}

export function usePageTrash() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: docs.docsKeys.trash(workspaceId), queryFn: () => docs.trash(client, workspaceId) })
}

export function useTrashedPage(pageId: string, enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ enabled, retry: false, queryKey: docs.docsKeys.trashedPage(workspaceId, pageId), queryFn: () => docs.trashedPage(client, workspaceId, pageId) })
}

export function useTrashWrites() {
  const { client, workspaceId } = useOrbit()
  const refresh = useRefreshDocs()
  return {
    restore: useMutation({ mutationFn: (page: Pick<docs.TrashedPage, 'id' | 'version'>) => docs.restoreFromTrash(client, workspaceId, page), onSettled: refresh }),
    purge: useMutation({ mutationFn: (page: Pick<docs.TrashedPage, 'id' | 'version'>) => docs.deleteForever(client, workspaceId, page), onSettled: refresh }),
    empty: useMutation({ mutationFn: () => docs.emptyTrash(client, workspaceId), onSettled: refresh }),
  }
}

export function useTeamspaceWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => {
    void queryClient.invalidateQueries({ queryKey: keys.teamspaces(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: keys.pages(workspaceId) })
  }
  return {
    create: useMutation({ mutationFn: (name: string) => docs.newTeamspace(client, workspaceId, name), onSettled }),
    save: useMutation({ mutationFn: ({ teamspace, ...body }: { teamspace: Teamspace; name?: string; icon?: string | null; project_id?: string | null }) => docs.saveTeamspace(client, workspaceId, teamspace, body), onSettled }),
    remove: useMutation({ mutationFn: (teamspace: Teamspace) => docs.removeTeamspace(client, workspaceId, teamspace), onSettled }),
    move: useMutation({ mutationFn: ({ teamspace, position }: { teamspace: Teamspace; position: number }) => docs.placeTeamspace(client, workspaceId, teamspace, position), onSettled }),
  }
}

export function usePageThreads(pageId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: docs.docsKeys.threads(workspaceId, pageId), queryFn: () => docs.threads(client, workspaceId, pageId) })
}

/** A new comment on the page (`threadId` unset) or a reply in a thread, as plain paragraphs. */
export function useAddComment(pageId: string) {
  const { client, workspaceId, session } = useOrbit()
  const queryClient = useQueryClient()
  const people = useMentionPeople()
  const keys = (useProjects().data ?? []).map((project) => project.key)
  return useMutation({
    mutationFn: async ({ text, threadId }: { text: string; threadId?: string }) => {
      // a typed task ID goes as a link to the task, as the web's "#" inserts it
      const body = paragraphBlocks(text, undefined, people, { keys, server: session.server })
      if (threadId) await docs.replyToThread(client, workspaceId, pageId, threadId, body)
      else await docs.commentOnPage(client, workspaceId, pageId, body)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: docs.docsKeys.threads(workspaceId, pageId) }),
  })
}
