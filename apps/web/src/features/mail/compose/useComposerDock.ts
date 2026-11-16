// The dock's state for the rest of the app: the store of each workspace, opening a new message, and where Undo
// brings a draft back.
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { MailDraft } from '@orbit/api/generated/types.gen'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCreateDraft } from '../api/mutations'
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

// The inline replies on screen, by thread: Undo brings a reply back where it was written when that thread is still
// open, and in a window otherwise.
const inlineHosts = new Map<string, (draftId: string) => void>()

/** Registers the inline reply of `threadId` while it is mounted. */
export function useInlineReplyHost(threadId: string, show: (draftId: string) => void) {
  const latest = useRef(show)
  useEffect(() => {
    latest.current = show
  })
  useEffect(() => {
    const host = (draftId: string) => latest.current(draftId)
    inlineHosts.set(threadId, host)
    return () => {
      if (inlineHosts.get(threadId) === host) inlineHosts.delete(threadId)
    }
  }, [threadId])
}

/** Shows a draft that Undo gave back: in its thread's inline reply when that is on screen, else in a window. */
export function reopenDraft(draft: MailDraft, store: ComposerDockStore) {
  const host = draft.thread_id && !store.has(draft.id) ? inlineHosts.get(draft.thread_id) : undefined
  if (host) host(draft.id)
  else store.open(draft.id)
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
