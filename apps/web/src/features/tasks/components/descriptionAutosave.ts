import type { RichTextDocument } from '../../../components/editor/document'

/** Long enough that a normal typing burst is one write, short enough to feel automatic. */
export const DESCRIPTION_AUTOSAVE_MS = 800

/**
 * Debounced description saves. Every write goes through `expected_version`, so
 * coalescing keystrokes is not just a network optimisation — it keeps the
 * version the editor holds from going stale between adjacent characters.
 *
 * - `change` restarts the debounce; `flush` saves now (blur, unmount).
 * - A document equal to the last one saved (or to `initial`) is never re-sent.
 * - While a save is in flight the next one waits and only the newest document
 *   goes out, so two writes never race on the same expected version.
 * - A failed save forgets what it sent, so the same document can be retried.
 */
export function createAutosave(
  save: (document: RichTextDocument) => void | Promise<unknown>,
  delay: number = DESCRIPTION_AUTOSAVE_MS,
  initial?: RichTextDocument,
) {
  let timer: ReturnType<typeof setTimeout> | undefined
  let lastSaved = initial ? JSON.stringify(initial) : undefined
  let inFlight = false
  let queued: RichTextDocument | undefined

  const commit = (document: RichTextDocument) => {
    if (inFlight) {
      queued = document
      return
    }
    const serialized = JSON.stringify(document)
    if (serialized === lastSaved) return
    lastSaved = serialized
    const result = save(document)
    if (!result) return
    inFlight = true
    void result
      .catch(() => {
        if (lastSaved === serialized) lastSaved = undefined
      })
      .finally(() => {
        inFlight = false
        const next = queued
        queued = undefined
        if (next) commit(next)
      })
  }

  const clear = () => {
    if (timer) clearTimeout(timer)
    timer = undefined
  }

  return {
    change(document: RichTextDocument) {
      clear()
      timer = setTimeout(() => {
        timer = undefined
        commit(document)
      }, delay)
    },
    flush(document: RichTextDocument) {
      clear()
      commit(document)
    },
    cancel() {
      clear()
      queued = undefined
    },
  }
}
