// BlockNote + ProseMirror are ~1 MB: a separate chunk, loaded when the browser is idle (see `preloadPageEditorWhenIdle`).
export const loadPageEditor = () => import('./PageEditor')

/** Starts the editor and Yjs downloads before a page (and its socket) is open. A failure surfaces again when they load. */
export function preloadPageEditor() {
  Promise.all([loadPageEditor(), import('yjs'), import('y-websocket')]).catch(() => {})
}

/**
 * The signed-in app: the editor is used everywhere (docs, task descriptions and comments, the new task dialog, which
 * opens from any page), and loading it on first use takes about a second. Starts the download once the browser is
 * idle after the app shows. Returns a cleanup that cancels a download that has not started.
 */
export function preloadPageEditorWhenIdle(): () => void {
  if (typeof window.requestIdleCallback === 'function') {
    const id = window.requestIdleCallback(preloadPageEditor, { timeout: 5000 })
    return () => window.cancelIdleCallback(id)
  }
  // Safari has no requestIdleCallback
  const id = window.setTimeout(preloadPageEditor, 2000)
  return () => window.clearTimeout(id)
}

