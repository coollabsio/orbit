import type { ComponentProps } from 'react'
import {
  Copy,
  Edit,
  Forward,
  Link2,
  Message as MessageIcon,
  Pin,
  PinOff,
  Reply,
  SmileCircle,
  TaskSquare,
  Trash,
  Unread,
  type IconComponent,
} from 'reicon-react'
import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import type { DropdownMenuContent } from '@/components/ui/dropdown-menu'
import { useChatContext } from '../../api/chatContext'
import { useDeleteMessage, useMarkUnread, useSetPinned, useToggleReaction } from '../../api/mutations'
import type { Message } from '../../api/types'
import { useChatHost } from '../../chatHost'
import { messagePath } from '../../chatRoutes'
import { canEdit, canForward } from '../../lib/forward'
import { decodeMentions } from '../../lib/mentionTokens'
import { useChatNavigation } from '../../useChatNavigation'
import { useChatPeople } from './people'

export type MenuAnchor = NonNullable<ComponentProps<typeof DropdownMenuContent>['anchor']>

/** A zero-size anchor at the pointer, so a right-click menu opens with its corner at the click. */
export function pointAnchor(x: number, y: number): MenuAnchor {
  return { getBoundingClientRect: () => DOMRect.fromRect({ x, y, width: 0, height: 0 }) }
}

export interface MessageAction {
  key: string
  label: string
  icon: IconComponent
  destructive?: boolean
  run: () => void
}

interface MessageActionOptions {
  /** In a thread there is no "Reply in Thread". */
  inThread: boolean
  /** Makes the message the composer's reply target. Absent where the user cannot write: no "Reply" then. */
  onReply?: (message: Message) => void
  /** Opens the forward dialog for the message. */
  onForward: (message: Message) => void
  onAddReaction: (message: Message) => void
  onEdit: (message: Message) => void
  /** Called before "Mark as unread" is sent, so the view stops marking itself read before the new state arrives. */
  onMarkUnread?: (message: Message) => void
}

/**
 * What a member can do with a message. The `…` menu, the right-click menu and the mobile action sheet all show
 * `actionsFor(message)`, so they never differ.
 */
export function useMessageActions({ inThread, onReply, onForward, onAddReaction, onEdit, onMarkUnread }: MessageActionOptions) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const host = useChatHost()
  const { openConversation, openThreadPane, showInThread } = useChatNavigation()
  const { mutate: toggleReaction } = useToggleReaction()
  const { mutate: setPinned } = useSetPinned()
  const { mutate: markUnread } = useMarkUnread()
  const { mutate: deleteMessage } = useDeleteMessage()
  const role = currentUserId ? people.byId.get(currentUserId)?.role : undefined
  const moderator = role === 'Owner' || role === 'Admin'

  function react(message: Message, emoji: string) {
    toggleReaction({ message, emoji }, { onError: () => toast.error('Could not update the reaction. Try again.') })
  }

  /** Opens the thread of a root, or the thread a reply belongs to (at that reply). */
  function openThread(message: Message) {
    if (message.threadRootId) openThreadPane(message.conversationId, message.threadRootId, message.id)
    else openThreadPane(message.conversationId, message.id)
  }

  /** Jumps to the message that a reply quotes: in the open thread for one of its replies, else in the conversation. */
  function openQuoted(message: Message) {
    if (!message.replyToId) return
    if (inThread && message.threadRootId) showInThread(message.threadRootId, message.replyToId)
    else openConversation(message.conversationId, message.replyToId)
  }

  /** Jumps to the message that a forward is a copy of, in its own conversation. */
  function openOrigin(message: Message) {
    if (message.forwarded) openConversation(message.forwarded.conversationId, message.forwarded.messageId)
  }

  function copy(text: string, done: string) {
    navigator.clipboard.writeText(text).then(
      () => toast.success(done),
      () => toast.error('Could not copy. Try again.'),
    )
  }

  async function remove(message: Message) {
    const confirmed = await confirmAction({
      title: 'Delete message?',
      description: message.replyCount > 0 ? 'The replies stay in the thread. This cannot be undone.' : 'This cannot be undone.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (confirmed) deleteMessage(message.id, { onError: () => toast.error('Could not delete the message. Try again.') })
  }

  function actionsFor(message: Message): MessageAction[] {
    // A message the server has not confirmed has no id to act on; its row offers retry and delete instead.
    if (message.sendState || message.kind !== 'message') return []
    const own = message.authorId === currentUserId
    const actions: MessageAction[] = []
    if (!message.deleted) {
      actions.push({ key: 'react', label: 'Add reaction', icon: SmileCircle, run: () => onAddReaction(message) })
    }
    if (onReply && !message.deleted) {
      actions.push({ key: 'reply', label: 'Reply', icon: Reply, run: () => onReply(message) })
    }
    if (!inThread) {
      actions.push({ key: 'thread', label: 'Reply in Thread', icon: MessageIcon, run: () => openThread(message) })
    }
    if (canForward(message)) {
      actions.push({ key: 'forward', label: 'Forward', icon: Forward, run: () => onForward(message) })
    }
    actions.push({
      key: 'link',
      label: 'Copy link',
      icon: Link2,
      run: () => copy(`${window.location.origin}${messagePath(message)}`, 'Link copied'),
    })
    if (message.deleted) return actions
    actions.push(
      {
        key: 'text',
        label: 'Copy text',
        icon: Copy,
        run: () => copy(decodeMentions(message.body, people.members, people.channels), 'Text copied'),
      },
      { key: 'task', label: 'Create task', icon: TaskSquare, run: () => host.createTask(message) },
      {
        key: 'pin',
        label: message.pinned ? 'Unpin' : 'Pin',
        icon: message.pinned ? PinOff : Pin,
        run: () =>
          setPinned(
            { message, pinned: !message.pinned },
            { onError: () => toast.error(message.pinned ? 'Could not unpin the message. Try again.' : 'Could not pin the message. Try again.') },
          ),
      },
      {
        key: 'unread',
        label: 'Mark as unread',
        icon: Unread,
        run: () => {
          onMarkUnread?.(message)
          markUnread(message.id, { onError: () => toast.error('Could not mark as unread. Try again.') })
        },
      },
    )
    if (canEdit(message, currentUserId)) actions.push({ key: 'edit', label: 'Edit', icon: Edit, run: () => onEdit(message) })
    if (own || moderator) {
      actions.push({ key: 'delete', label: 'Delete', icon: Trash, destructive: true, run: () => void remove(message) })
    }
    return actions
  }

  return { actionsFor, react, openThread, openQuoted, openOrigin }
}
