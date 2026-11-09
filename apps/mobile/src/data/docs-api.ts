/**
 * The Docs writes (create, rename, icon, cover, favorites, trash, teamspaces) and page comments over the
 * generated SDK. No React; the tree and block logic is in `docs.ts`.
 */
import {
  addPageFavorite,
  createPage,
  createPageComment,
  createPageThread,
  createTeamspace,
  deletePage,
  deleteTeamspace,
  duplicatePage,
  emptyPageTrash,
  listPageFavorites,
  listPageThreads,
  listPageTrash,
  moveTeamspace,
  purgePage,
  removePageFavorite,
  restorePage,
  socketTicket,
  updatePage,
  updateTeamspace,
  uploadPageFile,
} from '@orbit/api/generated/sdk.gen'
import type { CreatePageBody, PageFavorite, PageThread, PageUpdateBody, TrashedPage } from '@orbit/api/generated/types.gen'
import type { TicketResult } from '@orbit/editor/collab/ticketSocket'

import { multipart, type UploadFile } from './account-api'
import type { OrbitClient, Page, PageSummary, Teamspace } from './orbit'

export type { PageThread, TrashedPage }

/** Under the `pages` and `page` prefixes, so the workspace events socket refreshes them with the tree. */
export const docsKeys = {
  favorites: (workspaceId: string) => ['pages', workspaceId, 'favorites'] as const,
  trash: (workspaceId: string) => ['pages', workspaceId, 'trash'] as const,
  threads: (workspaceId: string, pageId: string) => ['page', workspaceId, pageId, 'threads'] as const,
}

const pagePath = (workspaceId: string, pageId: string) => ({ workspace_id: workspaceId, page_id: pageId })

export async function newPage(client: OrbitClient, workspaceId: string, body: CreatePageBody): Promise<Page> {
  return (await createPage({ client, path: { workspace_id: workspaceId }, body, throwOnError: true })).data
}

/**
 * Title, icon or cover. Never the content: the page body is a live collaborative document, edited only through the
 * page editor's socket (a content PATCH would replace everyone's edits).
 */
export async function savePage(client: OrbitClient, workspaceId: string, page: Pick<PageSummary, 'id' | 'version'>, body: Pick<PageUpdateBody, 'title' | 'icon' | 'cover_url'>): Promise<Page> {
  return (await updatePage({ client, path: pagePath(workspaceId, page.id), body: { ...body, expected_version: page.version }, throwOnError: true })).data
}

/** A copy of the page next to it, with its sub-pages when `withChildren`. */
export async function duplicate(client: OrbitClient, workspaceId: string, pageId: string, withChildren: boolean): Promise<Page> {
  return (await duplicatePage({ client, path: pagePath(workspaceId, pageId), body: { include_children: withChildren }, throwOnError: true })).data
}

/** Stores an image on the page; its `url` can go straight into `cover_url`. */
export async function uploadPageImage(client: OrbitClient, workspaceId: string, pageId: string, file: UploadFile): Promise<string> {
  return (await uploadPageFile({ client, path: pagePath(workspaceId, pageId), body: { file: file as Blob }, bodySerializer: multipart(file), throwOnError: true })).data.url
}

/**
 * A one-time ticket for the page editor's socket (`…/collab?ticket=`), which cannot send the bearer token. It opens
 * only the socket of `pageId`.
 */
export async function collabTicket(client: OrbitClient, pageId: string): Promise<TicketResult> {
  try {
    return { ok: true, ticket: (await socketTicket({ client, body: { kind: 'page', id: pageId }, throwOnError: true })).data.ticket }
  } catch (error) {
    const status = (error as { status?: unknown }).status
    return { ok: false, status: typeof status === 'number' ? status : 0 }
  }
}

/** An Orbit file (`/api/…`) an admin deleted: the server answers 410. */
export async function fileDeleted(server: string, token: string, url: string): Promise<boolean> {
  if (!url.startsWith('/api/')) return false
  try {
    const response = await fetch(`${server}${url}`, { method: 'HEAD', headers: { Authorization: `Bearer ${token}` } })
    return response.status === 410
  } catch {
    return false
  }
}

/** An Orbit file (`/api/…`, which needs the session) as a data URL, for the page editor's web view; `null` when it fails. */
export async function fileDataUrl(server: string, token: string, url: string): Promise<string | null> {
  if (!url.startsWith('/api/')) return null
  try {
    const response = await fetch(`${server}${url}`, { headers: { Authorization: `Bearer ${token}` } })
    // 410: an admin deleted the file, and the body is a "Deleted file" image to show in its place
    if (!response.ok && response.status !== 410) return null
    const blob = await response.blob()
    return await new Promise((resolve) => {
      const reader = new FileReader()
      reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : null)
      reader.onerror = () => resolve(null)
      reader.readAsDataURL(blob)
    })
  } catch {
    return null
  }
}

/** The favorites with what the list shows of each page, in the user's order. */
export async function favorites(client: OrbitClient, workspaceId: string): Promise<PageFavorite[]> {
  const { data } = await listPageFavorites({ client, path: { workspace_id: workspaceId }, throwOnError: true })
  return [...data.items].sort((a, b) => a.position - b.position)
}

export async function setFavorite(client: OrbitClient, workspaceId: string, pageId: string, favorite: boolean): Promise<void> {
  const options = { client, path: pagePath(workspaceId, pageId), throwOnError: true as const }
  if (favorite) await addPageFavorite(options)
  else await removePageFavorite(options)
}

/** Moves the page and its sub-pages to the trash. */
export async function trashPage(client: OrbitClient, workspaceId: string, page: Pick<PageSummary, 'id' | 'version'>): Promise<void> {
  await deletePage({ client, path: pagePath(workspaceId, page.id), query: { expected_version: page.version }, throwOnError: true })
}

/** Pages trashed directly, newest first. */
export async function trash(client: OrbitClient, workspaceId: string): Promise<TrashedPage[]> {
  const { data } = await listPageTrash({ client, path: { workspace_id: workspaceId }, throwOnError: true })
  return [...data.items].sort((a, b) => b.deleted_at.localeCompare(a.deleted_at))
}

export async function restoreFromTrash(client: OrbitClient, workspaceId: string, page: Pick<TrashedPage, 'id' | 'version'>): Promise<Page> {
  return (await restorePage({ client, path: pagePath(workspaceId, page.id), body: { expected_version: page.version }, throwOnError: true })).data
}

export async function deleteForever(client: OrbitClient, workspaceId: string, page: Pick<TrashedPage, 'id' | 'version'>): Promise<void> {
  await purgePage({ client, path: pagePath(workspaceId, page.id), query: { expected_version: page.version }, throwOnError: true })
}

/** Deletes forever every trashed page the caller may purge; answers how many went. */
export async function emptyTrash(client: OrbitClient, workspaceId: string): Promise<number> {
  return (await emptyPageTrash({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data.purged
}

export async function newTeamspace(client: OrbitClient, workspaceId: string, name: string): Promise<Teamspace> {
  return (await createTeamspace({ client, path: { workspace_id: workspaceId }, body: { name }, throwOnError: true })).data
}

export async function saveTeamspace(client: OrbitClient, workspaceId: string, teamspace: Pick<Teamspace, 'id' | 'version'>, body: { name?: string; icon?: string | null }): Promise<Teamspace> {
  return (await updateTeamspace({ client, path: { workspace_id: workspaceId, teamspace_id: teamspace.id }, body: { ...body, expected_version: teamspace.version }, throwOnError: true })).data
}

/** Moves a teamspace to `position` (its index among the teamspaces). */
export async function placeTeamspace(client: OrbitClient, workspaceId: string, teamspace: Pick<Teamspace, 'id' | 'version'>, position: number): Promise<void> {
  await moveTeamspace({ client, path: { workspace_id: workspaceId, teamspace_id: teamspace.id }, body: { expected_version: teamspace.version, position }, throwOnError: true })
}

export async function removeTeamspace(client: OrbitClient, workspaceId: string, teamspace: Pick<Teamspace, 'id' | 'version'>): Promise<void> {
  await deleteTeamspace({ client, path: { workspace_id: workspaceId, teamspace_id: teamspace.id }, query: { expected_version: teamspace.version }, throwOnError: true })
}

/** The page's comment threads, open and resolved. */
export async function threads(client: OrbitClient, workspaceId: string, pageId: string): Promise<PageThread[]> {
  return (await listPageThreads({ client, path: pagePath(workspaceId, pageId), throwOnError: true })).data.items
}

/** A comment on the whole page: a thread without a quoted passage (the web lists it with the page's other threads). */
export async function commentOnPage(client: OrbitClient, workspaceId: string, pageId: string, body: Record<string, unknown>[]): Promise<PageThread> {
  return (await createPageThread({ client, path: pagePath(workspaceId, pageId), body: { body, quote: '' }, throwOnError: true })).data
}

/** A plain-text reply: one comment paragraph per line, as the web's comment editor writes them. */
export async function replyToThread(client: OrbitClient, workspaceId: string, pageId: string, threadId: string, body: Record<string, unknown>[]): Promise<void> {
  await createPageComment({ client, path: { ...pagePath(workspaceId, pageId), thread_id: threadId }, body: { body }, throwOnError: true })
}
