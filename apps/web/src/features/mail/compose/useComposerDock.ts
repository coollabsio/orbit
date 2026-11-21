// The dock's state for the rest of the app: the store of each workspace and opening a new message.
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { MailFolder, MailThreadDetail } from '@orbit/api/generated/types.gen'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { mailKeys } from '../api/keys'
import { useCreateDraft, useUpdateThread } from '../api/mutations'
import { fromAddressesQuery } from '../api/queries'
import { problemText } from '../mailLib'
import { createComposerDockStore, type ComposerDockStore } from './composerDockStore'

// One dock for each workspace, shared by the shell (which shows it) and the mail page (which opens windows in it).
const stores = new Map<string, ComposerDockStore>()

function browserStorage(): Storage | null {
  try {
    return window.localStorage
  } catch {
    return null
  }
}

export function dockStoreFor(workspaceId: string): ComposerDockStore {
  let store = stores.get(workspaceId)
  if (!store) {
    store = createComposerDockStore({ workspaceId, storage: browserStorage() })
    stores.set(workspaceId, store)
  }
  return store
}

/** The workspace's dock: its state (live) and its actions. */
export function useComposerDock() {
  const store = dockStoreFor(useWorkspace().workspace.id)
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot)
  return { state, store }
}

// The mail page's open thread: ⌘⇧Enter on a reply to it archives it and opens the next one there.
type ArchiveNext = () => void
const archiveHosts = new Map<string, ArchiveNext>()

/** The mail page registers its open thread (`null`: none) while it shows it. */
export function useArchiveNextHost(threadId: string | null, archiveNext: ArchiveNext) {
  const latest = useRef(archiveNext)
  useEffect(() => {
    latest.current = archiveNext
  })
  useEffect(() => {
    if (!threadId) return
    const host = () => latest.current()
    archiveHosts.set(threadId, host)
    return () => {
      if (archiveHosts.get(threadId) === host) archiveHosts.delete(threadId)
    }
  }, [threadId])
}

/**
 * After ⌘⇧Enter: archives the thread. When the mail page shows that thread, the next one opens there; otherwise
 * (another page, another thread, the page moved on since Send) it is only archived. Returns the undo: Undo send puts
 * the thread back in the folder it was in.
 */
export function useArchiveAfterSend() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspace().workspace.id
  const update = useUpdateThread()
  // the promise form: its error still shows when the composer that sent is gone (mutate's callbacks would not run)
  const mutate = useRef(update.mutateAsync)
  useEffect(() => {
    mutate.current = update.mutateAsync
  })
  return useCallback(
    (threadId: string): (() => void) => {
      const before: MailFolder | undefined = queryClient.getQueryData<MailThreadDetail>(mailKeys.thread(workspaceId, threadId))?.thread.folder
      const move = (folder: MailFolder) =>
        mutate.current({ threadId, change: { folder } }).catch((error: unknown) => toast.error(problemText(error, 'The conversation could not be moved.')))
      const host = archiveHosts.get(threadId)
      if (host) host()
      else void move('archive')
      return () => {
        if (before !== 'archive') void move(before ?? 'inbox')
      }
    },
    [queryClient, workspaceId],
  )
}

/** Starts a new message from the user's first address that can send (else the first: Send then says why) in a window. */
export function useNewEmail() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspace().workspace.id
  const create = useCreateDraft()
  const busy = useRef(false)
  const createAsync = useRef(create.mutateAsync)
  useEffect(() => {
    createAsync.current = create.mutateAsync
  })
  return useCallback(async () => {
    if (busy.current) return
    busy.current = true
    try {
      const addresses = await queryClient.fetchQuery({ ...fromAddressesQuery(workspaceId), staleTime: 60_000 })
      const from = addresses.find((address) => address.ready === 'ready') ?? addresses[0]
      if (!from) {
        toast.error('You have no address to send from. Ask an admin for a mailbox.')
        return
      }
      const draft = await createAsync.current({ mode: 'new', from_address_id: from.address_id })
      dockStoreFor(workspaceId).open(draft.id)
    } catch (error) {
      toast.error(problemText(error, 'The new message could not be started.'))
    } finally {
      busy.current = false
    }
  }, [queryClient, workspaceId])
}
