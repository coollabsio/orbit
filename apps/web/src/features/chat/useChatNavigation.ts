import { useLocation, useNavigate } from 'react-router'
import { conversationPath, parseChatLocation, threadPath, type ChatLocation, type ChatPane } from './chatRoutes'
import { setStickyPane, useStickyPane } from './lib/stickyPane'
import { useIsPhone } from './pages/useChatViewport'

/** Router state of a full-view thread: where its close button and `Esc` go back to. */
interface ThreadOrigin {
  chatOrigin?: string
}

export function useChatLocation(): ChatLocation {
  const { pathname, search } = useLocation()
  return parseChatLocation(pathname, search)
}

/**
 * The pane (members, pins, files, threads) that is open beside the conversation: the one the URL names, else the one
 * the user left open. `null` while a thread or a search has its place. On a phone a pane is a sheet over the
 * conversation, so only the URL opens one there.
 */
export function useOpenPane(): ChatPane | null {
  const here = useChatLocation()
  const sticky = useStickyPane()
  const phone = useIsPhone()
  if (here.view !== 'conversation' || here.thread || here.q) return null
  return here.pane ?? (phone ? null : sticky)
}

/**
 * Every chat navigation, so the history rules are in one place: opening a conversation, a pane or a full thread pushes;
 * closing, expanding and collapsing replace.
 */
export function useChatNavigation() {
  const navigate = useNavigate()
  const location = useLocation()
  const here = parseChatLocation(location.pathname, location.search)
  const conversationId = here.view === 'conversation' || here.view === 'thread' ? here.conversationId : null
  const openPane = useOpenPane()

  return {
    openConversation(id: string, messageId?: string | null) {
      navigate(conversationPath(id, { m: messageId }))
    },
    /** Opens the thread in the right pane of its conversation. It replaces any other pane. */
    openThreadPane(id: string, rootId: string, messageId?: string | null) {
      navigate(conversationPath(id, { thread: rootId, m: messageId }))
    },
    /** Scrolls the open thread to one of its messages, in the pane or the full view it is in. */
    showInThread(rootId: string, messageId: string) {
      if (!conversationId) return
      if (here.view === 'thread') navigate(threadPath(conversationId, rootId, messageId), { state: location.state })
      else navigate(conversationPath(conversationId, { thread: rootId, m: messageId }))
    },
    /** Opens the thread in full view, and remembers this page as the place to close to. */
    openThreadFull(id: string, rootId: string, messageId?: string | null) {
      navigate(threadPath(id, rootId, messageId), { state: { chatOrigin: location.pathname + location.search } satisfies ThreadOrigin })
    },
    expandThread(rootId: string) {
      if (!conversationId) return
      navigate(threadPath(conversationId, rootId), { replace: true, state: location.state })
    },
    collapseThread(rootId: string) {
      if (!conversationId) return
      navigate(conversationPath(conversationId, { thread: rootId }), { replace: true })
    },
    /** Closes a full-view thread to where it was opened from; a direct URL load closes to the conversation. */
    closeThreadFull() {
      if (!conversationId) return
      const origin = (location.state as ThreadOrigin | null)?.chatOrigin
      navigate(origin ?? conversationPath(conversationId), { replace: true })
    },
    /** Shows a pane, or closes it when it is already open. An open pane stays open in every conversation. */
    togglePane(pane: ChatPane) {
      if (here.view !== 'conversation') return
      const open = openPane === pane
      setStickyPane(open ? null : pane)
      navigate(conversationPath(here.conversationId, { pane: open ? null : pane }), { replace: open || openPane !== null })
    },
    openSearch(query: string) {
      if (!conversationId) return
      navigate(conversationPath(conversationId, { q: query }), { replace: here.view === 'conversation' && Boolean(here.q) })
    },
    /** Closes the right pane and keeps the conversation. Behind a thread or a search, the pane the user left open shows again. */
    closePane() {
      if (!conversationId) return
      if (openPane) setStickyPane(null)
      navigate(conversationPath(conversationId), { replace: true })
    },
  }
}
