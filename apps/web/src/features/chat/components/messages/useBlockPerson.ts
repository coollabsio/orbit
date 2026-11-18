import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { useBlockedIds, useSetBlocked } from '../../api/moderation'

/** Block and unblock, for the message menu and the profile popover. A block asks first; an unblock does not. */
export function useBlockPerson() {
  const blocked = useBlockedIds()
  const { mutate: setBlocked, isPending } = useSetBlocked()

  async function block(userId: string, name: string) {
    const confirmed = await confirmAction({
      title: `Block ${name}?`,
      description: 'Chat hides their messages from you. They cannot send you direct messages. You can unblock them in Account settings.',
      confirmLabel: 'Block',
      danger: true,
    })
    if (!confirmed) return
    setBlocked(
      { userId, blocked: true },
      { onSuccess: () => toast.success(`You blocked ${name}.`), onError: () => toast.error(`Could not block ${name}. Try again.`) },
    )
  }

  function unblock(userId: string, name: string) {
    setBlocked(
      { userId, blocked: false },
      { onSuccess: () => toast.success(`You unblocked ${name}.`), onError: () => toast.error(`Could not unblock ${name}. Try again.`) },
    )
  }

  return { isBlocked: (userId: string) => blocked.has(userId), block, unblock, isPending }
}
