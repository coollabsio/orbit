import { Link } from 'react-router'
import { toast } from 'sonner'
import { TickCircle } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useMarkRead } from '@/features/chat/api/mutations'
import { flattenMessages, useConversations, useConversationStates, useMessages, useUnreadConversations } from '@/features/chat/api/queries'
import { conversationPath } from '@/features/chat/chatRoutes'
import { ChatBackLink } from '@/features/chat/components/sidebar/ChatRow'
import { ConversationIcon } from '@/features/chat/components/sidebar/ConversationIcon'
import { conversationTitle, type UnreadConversation } from '@/features/chat/lib/sidebar'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { Shortcut } from '@/shortcuts/Shortcut'
import { MessagePreview } from './MessagePreview'
import { useMarkAllReadWithUndo } from './useMarkAllReadWithUndo'

/** The newest unread messages shown for each conversation; the rest is one click away. */
const PREVIEW_LIMIT = 5

/** One conversation's unread messages (the newest few of the loaded page), with "Mark as read" and a link to it. */
function UnreadGroup({ conversation, state, people }: UnreadConversation & { people: readonly User[] }) {
  const { currentUserId } = useChatContext()
  const messages = useMessages(conversation.id)
  const markRead = useMarkRead()
  const title = conversationTitle(conversation, people, currentUserId ?? '')
  // message ids sort by time, so everything after the read cursor is unread
  const unread = flattenMessages(messages.data).filter(
    (message) => message.kind === 'message' && !message.deleted && message.authorId !== currentUserId && (state.lastReadMessageId === null || message.id > state.lastReadMessageId),
  )
  const shown = unread.slice(-PREVIEW_LIMIT)
  const more = Math.max(0, state.unreadCount - shown.length)

  return (
    <section data-slot="unread-group" aria-label={title} className="flex flex-col gap-3 border-b px-4 py-3 max-[899px]:px-3">
      <div className="flex min-w-0 items-center gap-2">
        <Link
          to={conversationPath(conversation.id)}
          className="flex min-w-0 items-center gap-2 rounded-sm text-[13px] font-semibold text-foreground outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          <ConversationIcon conversation={conversation} people={people} currentUserId={currentUserId} />
          <span className="truncate">{title}</span>
        </Link>
        <span className="shrink-0 text-xs text-muted-foreground">
          {state.unreadCount} unread
        </span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="ml-auto text-muted-foreground"
          disabled={markRead.isPending}
          onClick={() => markRead.mutate(conversation.id, { onError: () => void toast.error('Could not mark as read. Try again.') })}
        >
          Mark as read
        </Button>
      </div>
      {messages.isError ? (
        <p className="text-[13px] text-muted-foreground" role="alert">
          Could not load these messages.{' '}
          <Button type="button" variant="link" size="sm" className="h-auto p-0" onClick={() => void messages.refetch()}>
            Try again
          </Button>
        </p>
      ) : (
        <ol className="flex max-w-[90ch] flex-col gap-3" aria-label={`Unread messages in ${title}`}>
          {shown.map((message) => (
            <li key={message.id}>
              <MessagePreview message={message} people={people} />
            </li>
          ))}
        </ol>
      )}
      {more > 0 && !messages.isPending ? (
        <Link
          to={conversationPath(conversation.id)}
          className="self-start rounded-sm text-xs text-muted-foreground outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {more} more unread {more === 1 ? 'message' : 'messages'}
        </Link>
      ) : null}
    </section>
  )
}

/**
 * The Unreads view: one group for each unread conversation that is not muted, newest activity first. "Mark all as
 * read" also has `Shift+Esc` and offers Undo.
 */
export function UnreadsView() {
  const { workspaceId } = useChatContext()
  const conversations = useConversations()
  const states = useConversationStates()
  const groups = useUnreadConversations()
  const people = useMembers(workspaceId).data ?? []
  const markAllRead = useMarkAllReadWithUndo()
  const failed = conversations.isError || states.isError
  const loading = !failed && (conversations.data === undefined || states.data === undefined)

  return (
    <Pane data-slot="unreads-view">
      <PaneHeader className="max-[899px]:px-2">
        <ChatBackLink />
        <PaneTitle render={<h1 />} className="flex-1 text-sm">
          Unreads
        </PaneTitle>
        {groups.length > 0 ? (
          <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" onClick={markAllRead}>
            Mark all as read
            <Shortcut id="chat.markAllRead" className="max-[899px]:hidden" />
          </Button>
        ) : null}
      </PaneHeader>
      {failed ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-[13px] text-muted-foreground" role="alert">
          Could not load your unread messages.
          <Button type="button" variant="outline" size="sm" onClick={() => void Promise.all([conversations.refetch(), states.refetch()])}>
            Try again
          </Button>
        </div>
      ) : loading ? (
        <div className="flex-1" />
      ) : groups.length === 0 ? (
        <div className="flex flex-1 flex-col p-6">
          <EmptyState icon={TickCircle} title="You're all caught up" description="New messages in your channels and direct messages show here." />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          {groups.map((group) => (
            <UnreadGroup key={group.conversation.id} {...group} people={people} />
          ))}
        </div>
      )}
    </Pane>
  )
}
