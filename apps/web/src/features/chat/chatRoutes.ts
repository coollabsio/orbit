/** URL shapes of chat. Pure; the hooks that navigate are in `useChatNavigation.ts`. */

export const CHAT_PATH = '/chat'
export const UNREADS_PATH = '/chat/unreads'
export const THREADS_PATH = '/chat/threads'

/** Right panes that are not a thread. A thread (`?thread=`) and a search (`?q=`) take the pane before these. */
export type ChatPane = 'members' | 'pins' | 'files' | 'threads'
const PANES: readonly string[] = ['members', 'pins', 'files', 'threads']

export interface ConversationParams {
  /** Thread shown in the right pane. */
  thread?: string | null
  /** Message to scroll to and highlight. */
  m?: string | null
  /** Search query; shows the search pane. */
  q?: string | null
  pane?: ChatPane | null
}

function withQuery(path: string, params: Record<string, string | null | undefined>): string {
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) if (value) query.set(key, value)
  const text = query.toString()
  return text ? `${path}?${text}` : path
}

export function conversationPath(conversationId: string, params: ConversationParams = {}): string {
  return withQuery(`${CHAT_PATH}/${conversationId}`, { thread: params.thread, m: params.m, q: params.q, pane: params.pane })
}

/** The thread in full view. `m` is a reply to scroll to. */
export function threadPath(conversationId: string, rootId: string, m?: string | null): string {
  return withQuery(`${CHAT_PATH}/${conversationId}/thread/${rootId}`, { m })
}

/** The link that "Copy link" gives: the message in its conversation, or the reply in its thread. */
export function messagePath(message: { id: string; conversationId: string; threadRootId: string | null }): string {
  return message.threadRootId
    ? threadPath(message.conversationId, message.threadRootId, message.id)
    : conversationPath(message.conversationId, { m: message.id })
}

export type ChatLocation =
  | { view: 'home' }
  | { view: 'unreads' }
  | { view: 'threads' }
  | { view: 'conversation'; conversationId: string; thread: string | null; m: string | null; q: string | null; pane: ChatPane | null }
  | { view: 'thread'; conversationId: string; rootId: string; m: string | null }

/** Reads a chat URL. `unreads` and `threads` are reserved ids. */
export function parseChatLocation(pathname: string, search: string): ChatLocation {
  const [, root, first, second, third] = pathname.split('/')
  if (root !== 'chat' || !first) return { view: 'home' }
  if (first === 'unreads') return { view: 'unreads' }
  if (first === 'threads') return { view: 'threads' }
  const query = new URLSearchParams(search)
  if (second === 'thread' && third) return { view: 'thread', conversationId: first, rootId: third, m: query.get('m') }
  const pane = query.get('pane')
  return {
    view: 'conversation',
    conversationId: first,
    thread: query.get('thread'),
    m: query.get('m'),
    q: query.get('q'),
    pane: pane && PANES.includes(pane) ? (pane as ChatPane) : null,
  }
}
