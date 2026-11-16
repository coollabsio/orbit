// After Send (U4): the server holds the message for the user's undo time. A Sonner toast shows "Message sent" with
// **Undo (Z)** and a linear bar that runs out when the message goes; Undo stops it on the server and opens the
// composer again as it was. A scheduled message gets the same Undo for a few seconds. `Z` (outside text fields)
// undoes the newest send while its toast shows.
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import { toast } from 'sonner'
import type { MailDraft, MailOutbound } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { useCommand } from '@/shortcuts/useCommand'
import { useCancelOutbound } from '../api/mutations'
import { useMailSettingsMe } from '../api/queries'
import { problemText } from '../mailLib'
import { sendTimeLabel } from './composeLib'
import { UndoSendToast } from './UndoSendToast'
import { useMailTimeZone } from './useMailTimeZone'

/** Below this the message is as good as gone: no Undo is offered (an undo time of 0). */
const MIN_UNDO_MS = 1000
/** How long the toast of a scheduled message stays (its Undo works until the scheduled time). */
const SCHEDULED_TOAST_MS = 6000

// The sends that can be undone right now, newest last: what `Z` acts on.
interface PendingUndo {
  id: string
  undo: () => void
}
let pending: readonly PendingUndo[] = []
const listeners = new Set<() => void>()
const setPending = (next: readonly PendingUndo[]) => {
  pending = next
  for (const listener of listeners) listener()
}
const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => void listeners.delete(listener)
}
const snapshot = () => pending

/** `Z` undoes the newest send while its toast shows; the command (and its ⌘K entry) exists only then. */
export function useUndoSendShortcut() {
  const entries = useSyncExternalStore(subscribe, snapshot)
  useCommand('mail.undoSend', () => entries.at(-1)?.undo(), { enabled: entries.length > 0 })
}

/**
 * What a composer host does after a send: the toast with Undo. `reopen` gets the draft back after a successful
 * Undo and shows it where the user expects it (the inline reply, or a window).
 */
export function useAnnounceSent() {
  const cancel = useCancelOutbound()
  const cancelAsync = useRef(cancel.mutateAsync)
  useEffect(() => {
    cancelAsync.current = cancel.mutateAsync
  })
  const timeZone = useMailTimeZone()
  const undoSeconds = useMailSettingsMe().data?.undo_send_seconds

  return useCallback(
    (outbound: MailOutbound, reopen: (draft: MailDraft) => void) => {
      // the user's undo time; the server's release time caps it (it counts from the server's clock, before the
      // answer came back), unless the two clocks disagree so much that it is already past
      const serverLeft = Date.parse(outbound.release_at) - Date.now()
      const left = undoSeconds === undefined ? serverLeft : serverLeft > 0 ? Math.min(serverLeft, undoSeconds * 1000) : undoSeconds * 1000
      if (!outbound.scheduled && !(left >= MIN_UNDO_MS)) {
        toast.success('Message sent')
        return
      }
      const id = `mail-sent-${outbound.id}`
      let settled = false
      let timer: ReturnType<typeof setTimeout> | undefined
      // the toast leaves when the message goes (not on Sonner's timer: hovering pauses that one, the server does not wait)
      const finish = () => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        setPending(pending.filter((entry) => entry.id !== id))
        toast.dismiss(id)
      }
      const undo = () => {
        if (settled) return
        finish()
        cancelAsync.current(outbound.id).then(reopen, (error: unknown) =>
          toast.error(
            error instanceof ApiProblem && error.code === 'already_sent'
              ? 'Too late: the message was already sent.'
              : problemText(error, 'The message could not be stopped. Check Sent.'),
          ),
        )
      }
      timer = setTimeout(finish, outbound.scheduled ? SCHEDULED_TOAST_MS : left)
      setPending([...pending, { id, undo }])
      const title = outbound.scheduled ? `Scheduled for ${sendTimeLabel(new Date(outbound.release_at), new Date(), timeZone, true)}` : 'Message sent'
      toast.custom(() => <UndoSendToast title={title} countdownMs={outbound.scheduled ? null : left} onUndo={undo} />, {
        id,
        duration: Infinity,
        onDismiss: finish,
        // enters from the bottom in 200 ms on the strong ease-out (Sonner's own move is 400 ms)
        className: 'duration-200! ease-(--ease-out)!',
      })
    },
    [timeZone, undoSeconds],
  )
}
