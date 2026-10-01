import { useHref } from 'react-router'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useConversations, useFollowedThreads } from '@/features/chat/api/queries'
import type { Conversation, FollowedThread } from '@/features/chat/api/types'
import { threadPath } from '@/features/chat/chatRoutes'
import { ChatBackLink, CountBadge } from '@/features/chat/components/sidebar/ChatRow'
import { ConversationIcon } from '@/features/chat/components/sidebar/ConversationIcon'
import { conversationTitle, sortFollowedThreads } from '@/features/chat/lib/sidebar'
import { fullTimestamp, relativeAgo } from '@/features/chat/lib/time'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { MessagePreview } from './MessagePreview'

/**
 * One followed thread: its conversation, the root message, the last reply and the unread count. The whole row is one
 * link that opens the thread in full view (closing it comes back here).
 */
function ThreadRow({ thread, conversation, people }: { thread: FollowedThread; conversation: Conversation | undefined; people: readonly User[] }) {
  const { currentUserId } = useChatContext()
  const navigation = useChatNavigation()
  const href = useHref(threadPath(thread.conversationId, thread.root.id))
  const unread = thread.state.unreadReplies
  const title = conversation ? conversationTitle(conversation, people, currentUserId ?? '') : 'Unknown conversation'
  const lastReplyAt = thread.lastReply?.createdAt ?? thread.root.lastReplyAt
  const replies = `${thread.root.replyCount} ${thread.root.replyCount === 1 ? 'reply' : 'replies'}`

  return (
    <article
      data-slot="thread-row"
      data-unread={unread > 0}
      className="relative flex flex-col gap-2 border-b px-4 py-3 has-[a:focus-visible]:bg-muted/40 hover-fine:hover:bg-muted/40 max-[899px]:px-3"
    >
      <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
        <a
          href={href}
          aria-label={`Thread in ${title}, ${replies}${unread > 0 ? `, ${unread} unread` : ''}`}
          className="flex min-w-0 items-center gap-1.5 text-[13px] font-medium text-foreground outline-none after:absolute after:inset-0 in-data-[unread=true]:font-semibold"
          onClick={(event) => {
            // a modified click opens a new tab, like any link
            if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
            event.preventDefault()
            navigation.openThreadFull(thread.conversationId, thread.root.id)
          }}
        >
          {conversation ? <ConversationIcon conversation={conversation} people={people} currentUserId={currentUserId} size={16} /> : null}
          <span className="truncate">{title}</span>
        </a>
        <span className="shrink-0">{replies}</span>
        {lastReplyAt ? (
          <time className="shrink-0 max-[899px]:hidden" dateTime={new Date(lastReplyAt).toISOString()} title={fullTimestamp(lastReplyAt)}>
            last reply {relativeAgo(lastReplyAt)}
          </time>
        ) : null}
        {unread > 0 ? <CountBadge count={unread} className="ml-auto" /> : null}
      </div>
      <MessagePreview message={thread.root} people={people} clamp className="max-w-[90ch]" />
      {thread.lastReply ? <MessagePreview message={thread.lastReply} people={people} clamp className="max-w-[90ch] border-l-2 pl-3" /> : null}
    </article>
  )
}

/** The Threads view: the threads the user follows, unread first, then by last reply. */
export function ThreadsView() {
  const { workspaceId } = useChatContext()
  const threads = useFollowedThreads()
  const conversations = useConversations().data ?? []
  const people = useMembers(workspaceId).data ?? []
  const rows = sortFollowedThreads((threads.data ?? []).filter((thread) => thread.state.following))

  return (
    <Pane data-slot="threads-view">
      <PaneHeader className="max-[899px]:px-2">
        <ChatBackLink />
        <PaneTitle render={<h1 />} className="flex-1 text-sm">
          Threads
        </PaneTitle>
      </PaneHeader>
      {threads.isError ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-[13px] text-muted-foreground" role="alert">
          Could not load your threads.
          <Button type="button" variant="outline" size="sm" onClick={() => void threads.refetch()}>
            Try again
          </Button>
        </div>
      ) : threads.data === undefined ? (
        <div className="flex-1" />
      ) : rows.length === 0 ? (
        <div className="flex flex-1 flex-col p-6">
          <EmptyState icon={ThreadIcon} title="Threads you follow appear here" description="Reply to a message or follow its thread, and new replies show up in this list." />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          {rows.map((thread) => (
            <ThreadRow
              key={thread.root.id}
              thread={thread}
              conversation={conversations.find((conversation) => conversation.id === thread.conversationId)}
              people={people}
            />
          ))}
        </div>
      )}
    </Pane>
  )
}
