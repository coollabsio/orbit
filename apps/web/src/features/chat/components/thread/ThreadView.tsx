import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { useChatContext } from '../../api/chatContext'
import { chatKeys } from '../../api/keys'
import { useMarkThreadRead } from '../../api/mutations'
import { flattenMessages, useConversation, useThread, useThreadState, useUnconfirmedMessages } from '../../api/queries'
import type { Conversation, Message, ThreadState } from '../../api/types'
import { decodeMentions } from '../../lib/mentionTokens'
import { useChatNavigation } from '../../useChatNavigation'
import { Composer, type ComposerHandle } from '../composer/Composer'
import { ConnectionLine, TypingLine } from '../composer/StatusLines'
import { useReplyTarget } from '../composer/useReplyTarget'
import { useDelayed, useWindowFocused } from '../messages/environment'
import { restoreFocusAfterThread } from '../messages/focusReturn'
import { MessageList, type MessageListHandle } from '../messages/MessageList'
import { firstLine } from '../messages/messageText'
import { conversationTitle, useChatPeople } from '../messages/people'
import { notifyMarkedUnread } from '../messages/unreadSignal'
import { ThreadHeader } from './ThreadHeader'
import { forgetThreadScroll, keepThreadScroll, keptThreadScroll } from './threadScroll'

interface ThreadViewProps {
  conversationId: string
  rootId: string
  variant: 'pane' | 'full'
  /** A reply to scroll to and highlight. */
  focusMessageId?: string | null
  /** False hides expand and collapse (a pane-URL thread shown in the full layout on a narrow screen). */
  canResize?: boolean
}

interface ThreadBodyProps {
  conversation: Conversation
  root: Message
  /** The user's live state in this thread; absent when they neither follow it nor opened it before. */
  state: ThreadState | undefined
  focusMessageId?: string | null
  announce: boolean
  list: RefObject<MessageListHandle | null>
}

/** The messages and the composer. Mounted when the thread has loaded, so what it captures on open stays. */
function ThreadBody({ conversation, root, state, focusMessageId, announce, list }: ThreadBodyProps) {
  const thread = useThread(root.id)
  const { workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  const focused = useWindowFocused()
  const { mutate: markThreadRead } = useMarkThreadRead()
  const composer = useRef<ComposerHandle>(null)
  const replies = [...flattenMessages(thread.data), ...useUnconfirmedMessages(conversation.id, root.id)]
  const rootId = root.id

  // The read cursor at the moment of opening: the "New" line stays there while the thread is open.
  const [openCursor] = useState(() => (state && state.unreadReplies > 0 ? state.lastReadReplyId : undefined))
  const [initialFromBottom] = useState(() => keptThreadScroll(rootId))
  const [atBottom, setAtBottom] = useState(false)
  // "Mark as unread" on a reply stops automatic mark-read until the thread closes.
  const [suspended, setSuspended] = useState(false)
  const [replyTo, setReplyTo] = useReplyTarget()

  useEffect(() => forgetThreadScroll(rootId), [rootId])

  const unread = state !== undefined && (state.unreadReplies > 0 || state.mentionCount > 0)
  // The same rule as a conversation: open, the window has focus, and the list is at the newest reply.
  useEffect(() => {
    if (atBottom && focused && !suspended && unread) markThreadRead(rootId)
  }, [atBottom, focused, suspended, unread, state?.unreadReplies, markThreadRead, rootId])

  // A jump to a reply that is older than the loaded ones: load back until it is there.
  const focusLoaded = !focusMessageId || focusMessageId === rootId || replies.some((reply) => reply.id === focusMessageId)
  const { hasPreviousPage, isFetchingPreviousPage, isFetchPreviousPageError, fetchPreviousPage } = thread
  useEffect(() => {
    // A failed page stops the loop; without that it would ask again at once, without end.
    if (!focusLoaded && hasPreviousPage && !isFetchingPreviousPage && !isFetchPreviousPageError) void fetchPreviousPage({ cancelRefetch: false })
  }, [focusLoaded, hasPreviousPage, isFetchingPreviousPage, isFetchPreviousPageError, fetchPreviousPage])

  const canWrite = conversation.isMember && !conversation.archived

  return (
    <>
      <MessageList
        ref={list}
        label="Thread"
        inThread
        root={root}
        messages={replies}
        loading={false}
        hasOlder={thread.hasPreviousPage}
        hasNewer={thread.hasNextPage}
        loadingOlder={thread.isFetchingPreviousPage}
        loadingNewer={thread.isFetchingNextPage}
        onLoadOlder={() => void thread.fetchPreviousPage({ cancelRefetch: false })}
        onLoadNewer={() => void thread.fetchNextPage({ cancelRefetch: false })}
        lastReadMessageId={suspended ? state?.lastReadReplyId : openCursor}
        openAt="unread"
        initialFromBottom={initialFromBottom}
        focusMessageId={focusMessageId}
        announce={announce}
        onAtBottomChange={setAtBottom}
        // The newest replies left the list in a long scroll back: the thread loads again from its end.
        onJumpToLatest={() => void queryClient.resetQueries({ queryKey: chatKeys.thread(workspaceId, rootId), exact: true })}
        onMarkUnread={(message) => {
          // A reply moves the thread's cursor; the root moves the conversation's.
          if (message.threadRootId) setSuspended(true)
          else notifyMarkedUnread(conversation.id)
        }}
        onReply={canWrite ? setReplyTo : undefined}
        onFocusComposer={() => composer.current?.focus()}
      />
      {canWrite ? (
        <>
          <TypingLine conversationId={conversation.id} threadRootId={rootId} />
          <ConnectionLine />
          <Composer
            ref={composer}
            conversation={conversation}
            threadRootId={rootId}
            replyTo={replyTo}
            onClearReply={() => setReplyTo(null)}
            autoFocus
            onEditLast={() => list.current?.editLastOwn() ?? false}
            onFocusList={() => list.current?.focusLast() ?? false}
          />
        </>
      ) : (
        <p data-slot="thread-notice" className="shrink-0 border-t border-border px-4 py-3 text-sm text-muted-foreground max-[899px]:px-3">
          {conversation.archived ? 'This channel is archived. You can read it, but you cannot send messages.' : 'Join the channel to reply.'}
        </p>
      )}
    </>
  )
}

/**
 * A thread, in the right pane or in full view: its own 48px header, the root message, the replies and the thread
 * composer. It marks the thread read by the same rule as a conversation.
 */
export function ThreadView({ conversationId, rootId, variant, focusMessageId, canResize = true }: ThreadViewProps) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const { closePane, closeThreadFull } = useChatNavigation()
  const conversation = useConversation(conversationId).data
  const thread = useThread(rootId)
  const followed = useThreadState(rootId).data
  const list = useRef<MessageListHandle>(null)
  const page = thread.data?.pages[0]
  const root = page?.root
  const state = followed ?? page?.state ?? undefined
  const showSpinner = useDelayed(thread.isPending, 300)

  function close() {
    forgetThreadScroll(rootId)
    if (variant === 'pane') closePane()
    else closeThreadFull()
    // Focus goes back to the message the thread belongs to, or to the conversation's composer.
    restoreFocusAfterThread(rootId)
  }

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    // Popups (menus, the emoji picker, dialogs) are portals: their `Esc` is theirs. The mention list and an inline
    // edit stop the event before it gets here, so `Esc` closes one thing at a time and never clears a draft.
    if (event.key !== 'Escape' || event.defaultPrevented || !event.currentTarget.contains(event.target as Node)) return
    event.preventDefault()
    event.stopPropagation()
    close()
  }

  return (
    <section
      data-slot="thread-view"
      data-variant={variant}
      aria-label="Thread"
      // A size container: the composer grows to half of this column's height (`50cqh`).
      className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background [container-type:size]"
      onKeyDown={onKeyDown}
    >
      <ThreadHeader
        conversationId={conversationId}
        rootId={rootId}
        variant={variant}
        conversationTitle={conversation ? conversationTitle(conversation, people, currentUserId) : ''}
        rootLine={root && !root.deleted ? firstLine(decodeMentions(root.body, people.members, people.channels), 60) : ''}
        following={state?.following ?? false}
        canResize={canResize}
        onResize={() => keepThreadScroll(rootId, list.current?.distanceFromBottom() ?? 0)}
        onClose={close}
      />
      {root && conversation ? (
        <ThreadBody
          key={rootId}
          conversation={conversation}
          root={root}
          state={state}
          focusMessageId={focusMessageId}
          announce={variant === 'full'}
          list={list}
        />
      ) : thread.isError ? (
        <div role="alert" className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
          <p className="text-sm text-muted-foreground">Could not load this thread. It may have been deleted.</p>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => void thread.refetch()}>
              Try again
            </Button>
            <Button variant="ghost" onClick={close}>
              Close thread
            </Button>
          </div>
        </div>
      ) : (
        // First load: blank for 300ms, then a quiet spinner.
        <div aria-busy="true" className="flex flex-1 items-center justify-center">
          {showSpinner ? <Spinner className="text-muted-foreground" /> : null}
        </div>
      )}
    </section>
  )
}
