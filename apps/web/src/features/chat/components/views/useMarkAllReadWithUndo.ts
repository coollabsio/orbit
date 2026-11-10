import { toast } from 'sonner'
import { useMarkAllRead, useRestoreRead } from '@/features/chat/api/mutations'
import { UNDO_TOAST_DURATION } from '@/lib/toast'

/** "Mark all as read" for the Unreads view and `Shift+Esc`: every conversation, every followed thread, and every thread with unread replies in a conversation that is not muted, with an Undo toast. */
export function useMarkAllReadWithUndo(): () => void {
  const markAllRead = useMarkAllRead()
  const restoreRead = useRestoreRead()
  return () => {
    if (markAllRead.isPending) return
    markAllRead.mutate(undefined, {
      onSuccess: (previous) => {
        toast('Marked all as read', {
          duration: UNDO_TOAST_DURATION,
          action: {
            label: 'Undo',
            onClick: () => restoreRead.mutate(previous, { onError: () => void toast.error('Could not undo. Try again.') }),
          },
        })
      },
      onError: () => void toast.error('Could not mark all as read. Try again.'),
    })
  }
}
