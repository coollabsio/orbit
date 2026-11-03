/** The Docs writes, favorites, trash and comments, over `data/docs-api.ts`. The tree lists are in `src/queries.ts`. */
import { type QueryClient, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import type { UploadFile } from '../data/account-api'
import { paragraphBlocks, spaceRequest } from '../data/docs'
import * as docs from '../data/docs-api'
import { keys } from '../data/keys'
import type { Page, PageSummary, Teamspace } from '../data/orbit'
import { useOrbit } from '../orbit-context'

/** A saved page into its detail cache and the tree, without a refetch. */
function reconcile(queryClient: QueryClient, workspaceId: string, page: Page) {
  queryClient.setQueryData(keys.page(workspaceId, page.id), page)
  const summary: PageSummary = {
    id: page.id,
    parent_id: page.parent_id,
    teamspace_id: page.teamspace_id,
    private: page.private,
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
    mutationFn: ({ space = null, parentId }: { space?: string | null; parentId?: string }) =>
      docs.newPage(client, workspaceId, parentId ? { parent_id: parentId } : spaceRequest(space)),
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

/** Adds (appended last) or removes a favorite; the star moves at once and goes back on an error. */
export function useToggleFavorite() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const key = docs.docsKeys.favorites(workspaceId)
  return useMutation({
    mutationFn: ({ pageId, favorite }: { pageId: string; favorite: boolean }) => docs.setFavorite(client, workspaceId, pageId, favorite),
    onMutate: async ({ pageId, favorite }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const ids = queryClient.getQueryData<string[]>(key)
      const without = (ids ?? []).filter((id) => id !== pageId)
      queryClient.setQueryData<string[]>(key, favorite ? [...without, pageId] : without)
      return { ids }
    },
    onError: (_error, _input, context) => queryClient.setQueryData(key, context?.ids),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** Everything a trash change touches: the tree, the trash and the open pages. */
function useRefreshDocs() {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: keys.pages(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: ['page', workspaceId] })
  }
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

export function useTrashWrites() {
  const { client, workspaceId } = useOrbit()
  const refresh = useRefreshDocs()
  return {
    restore: useMutation({ mutationFn: (page: docs.TrashedPage) => docs.restoreFromTrash(client, workspaceId, page), onSettled: refresh }),
    purge: useMutation({ mutationFn: (page: docs.TrashedPage) => docs.deleteForever(client, workspaceId, page), onSettled: refresh }),
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
    save: useMutation({ mutationFn: ({ teamspace, ...body }: { teamspace: Teamspace; name?: string; icon?: string | null }) => docs.saveTeamspace(client, workspaceId, teamspace, body), onSettled }),
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
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ text, threadId }: { text: string; threadId?: string }) => {
      const body = paragraphBlocks(text)
      if (threadId) await docs.replyToThread(client, workspaceId, pageId, threadId, body)
      else await docs.commentOnPage(client, workspaceId, pageId, body)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: docs.docsKeys.threads(workspaceId, pageId) }),
  })
}
