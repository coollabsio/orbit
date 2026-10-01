import { useLocation, useNavigate } from 'react-router'
import { conversationPath, parseChatLocation, threadPath, type ChatLocation, type ChatPane } from './chatRoutes'

/** Router state of a full-view thread: where its close button and `Esc` go back to. */
interface ThreadOrigin {
  chatOrigin?: string
}

export function useChatLocation(): ChatLocation {
  const { pathname, search } = useLocation()
  return parseChatLocation(pathname, search)
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

  return {
    openConversation(id: string, messageId?: string | null) {
      navigate(conversationPath(id, { m: messageId }))
    },
    /** Opens the thread in the right pane of its conversation. It replaces any other pane. */
    openThreadPane(id: string, rootId: string, messageId?: string | null) {
      navigate(conversationPath(id, { thread: rootId, m: messageId }))
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
    /** Shows a pane, or closes it when it is already open. */
    togglePane(pane: ChatPane) {
      if (here.view !== 'conversation') return
      const open = !here.thread && !here.q && here.pane === pane
      navigate(conversationPath(here.conversationId, { pane: open ? null : pane }), { replace: open })
    },
    openSearch(query: string) {
      if (!conversationId) return
      navigate(conversationPath(conversationId, { q: query }), { replace: here.view === 'conversation' && Boolean(here.q) })
    },
    /** Closes the right pane (thread, search or other) and keeps the conversation. */
    closePane() {
      if (!conversationId) return
      navigate(conversationPath(conversationId), { replace: true })
    },
  }
}
