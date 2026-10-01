/**
 * "Mark as unread" on the root message inside a thread view changes the conversation's cursor, not the thread's. The
 * conversation column (open beside a thread pane) listens here, so it stops marking itself read as well.
 */
const listeners = new Set<(conversationId: string) => void>()

export function notifyMarkedUnread(conversationId: string) {
  for (const listener of listeners) listener(conversationId)
}

export function onMarkedUnread(listener: (conversationId: string) => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
