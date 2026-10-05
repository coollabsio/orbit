import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState, type DragEvent, type KeyboardEvent } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { useChatContext } from '../api/chatContext'
import { chatKeys } from '../api/keys'
import { useJoinChannel, useMarkRead } from '../api/mutations'
import { flattenMessages, useConversation, useConversationState, useMessages, useUnconfirmedMessages } from '../api/queries'
import type { Conversation, ConversationState } from '../api/types'
import { useChatLocation, useChatNavigation } from '../useChatNavigation'
import { Composer, type ComposerHandle } from './composer/Composer'
import { ConnectionLine, TypingLine } from './composer/StatusLines'
import { useReplyTarget } from './composer/useReplyTarget'
import { ConversationStart } from './messages/ConversationStart'
import { useWindowFocused } from './messages/environment'
import { MessageList, type MessageListHandle } from './messages/MessageList'
import { conversationTitle, useChatPeople } from './messages/people'
import { onMarkedUnread } from './messages/unreadSignal'

function hasFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer.types).includes('Files')
}

function JoinBar({ conversation, title }: { conversation: Conversation; title: string }) {
  const { mutate: join, isPending } = useJoinChannel()
  return (
    <div data-slot="join-bar" className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-border px-4 py-3 max-[899px]:px-3">
      <p className="text-sm text-muted-foreground">You are viewing {title}. Join it to send messages.</p>
      <Button
        disabled={isPending}
        onClick={() => join(conversation.id, { onError: () => toast.error(`Could not join ${title}. Try again.`) })}
      >
        Join {title}
      </Button>
    </div>
  )
}

interface ColumnProps {
  conversation: Conversation
  /** The user's live state; absent for a public channel the user has not joined. */
  state: ConversationState | undefined
  focusMessageId?: string | null
}

/** Mounted once for each opened conversation (keyed by id), so what it captures on open lasts until the user leaves. */
function ConversationColumn({ conversation, state, focusMessageId }: ColumnProps) {
  const { currentUserId, workspaceId } = useChatContext()
  const queryClient = useQueryClient()
  const people = useChatPeople()
  const location = useChatLocation()
  const { closePane } = useChatNavigation()
  const focused = useWindowFocused()
  const { mutate: markRead } = useMarkRead()
  const list = useRef<MessageListHandle>(null)
  const composer = useRef<ComposerHandle>(null)
  const conversationId = conversation.id
  const title = conversationTitle(conversation, people, currentUserId)

  // The read cursor at the moment of opening: the "New" line stays there while the conversation is open.
  const [openCursor] = useState(() => (state && state.unreadCount > 0 ? state.lastReadMessageId : undefined))
  // Which messages are loaded: a window around a message (a jump, the first unread), or the newest ones.
  const [view, setView] = useState<{ around: string | null; openAt: 'unread' | 'bottom'; turn: number }>(() => ({
    around: focusMessageId ?? (typeof openCursor === 'string' ? openCursor : null),
    openAt: focusMessageId ? 'bottom' : 'unread',
    turn: 0,
  }))
  const [seenFocus, setSeenFocus] = useState(focusMessageId)
  const [atBottom, setAtBottom] = useState(false)
  // "Mark as unread" stops automatic mark-read until the user leaves the conversation.
  const [suspended, setSuspended] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [replyTo, setReplyTo] = useReplyTarget()

  const query = useMessages(conversationId, { around: view.around })
  const messages = [...flattenMessages(query.data), ...useUnconfirmedMessages(conversationId)]

  /** Shows the newest messages. The list of the newest messages loads again if a long scroll back made it drop them. */
  function toLatest() {
    if (view.around === null) void queryClient.resetQueries({ queryKey: chatKeys.messages(workspaceId, conversationId), exact: true })
    setView({ around: null, openAt: 'bottom', turn: view.turn + 1 })
  }
  // The conversation was left far back in its history: it opens at the newest messages again.
  useEffect(() => {
    if (view.around === null && query.hasNextPage) toLatest()
    // Only for the list that the cache had at the moment of opening.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // A new `?m=` while the conversation is open: load around that message unless it is already here.
  if (focusMessageId !== seenFocus) {
    setSeenFocus(focusMessageId)
    if (focusMessageId && !messages.some((message) => message.id === focusMessageId)) {
      setView({ around: focusMessageId, openAt: 'bottom', turn: view.turn })
    }
  }

  // After "Mark as unread" the line follows the cursor the user chose.
  const cursor = suspended ? state?.lastReadMessageId : openCursor
  const unread = state !== undefined && (state.unreadCount > 0 || state.mentionCount > 0)
  const canWrite = conversation.isMember && !conversation.archived

  // Read means: open, the window has focus, and the list is at the newest message.
  useEffect(() => {
    if (conversation.isMember && atBottom && focused && !suspended && unread) markRead(conversationId)
  }, [conversation.isMember, atBottom, focused, suspended, unread, state?.unreadCount, markRead, conversationId])

  // The root message was marked as unread from its thread view.
  useEffect(() => onMarkedUnread((id) => id === conversationId && setSuspended(true)), [conversationId])

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    // Popups (menus, the emoji picker, dialogs) are portals: their `Esc` is theirs.
    if (event.key !== 'Escape' || event.defaultPrevented || !event.currentTarget.contains(event.target as Node)) return
    // Esc closes a thread or a search; the members, pins, files and threads panes stay until their own close button
    const paneOpen = location.view === 'conversation' && Boolean(location.thread || location.q)
    if (!paneOpen) return
    event.preventDefault()
    closePane()
  }

  function onDrop(event: DragEvent<HTMLElement>) {
    setDragging(false)
    if (!canWrite || !hasFiles(event)) return
    event.preventDefault()
    composer.current?.addFiles(Array.from(event.dataTransfer.files))
    composer.current?.focus()
  }

  return (
    <section
      data-slot="conversation-view"
      data-dragging={dragging ? '' : undefined}
      aria-label={title}
      // A size container: the composer grows to half of this column's height (`50cqh`).
      className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background [container-type:size]"
      onKeyDown={onKeyDown}
      onDragEnter={(event) => {
        if (canWrite && hasFiles(event)) setDragging(true)
      }}
      onDragOver={(event) => {
        if (canWrite && hasFiles(event)) event.preventDefault()
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragging(false)
      }}
      onDrop={onDrop}
    >
      {query.isError ? (
        <div role="alert" className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
          <p className="text-sm text-muted-foreground">Could not load the messages of {title}.</p>
          <Button variant="outline" onClick={() => void query.refetch()}>
            Try again
          </Button>
        </div>
      ) : (
        <MessageList
          key={`${view.around ?? 'latest'}-${view.turn}`}
          ref={list}
          label={`Messages in ${title}`}
          messages={messages}
          loading={query.isPending}
          hasOlder={query.hasPreviousPage}
          hasNewer={query.hasNextPage}
          loadingOlder={query.isFetchingPreviousPage}
          loadingNewer={query.isFetchingNextPage}
          // A page must not stop a refetch that is on its way (after a resync): the refetch would be lost.
          onLoadOlder={() => void query.fetchPreviousPage({ cancelRefetch: false })}
          onLoadNewer={() => void query.fetchNextPage({ cancelRefetch: false })}
          lastReadMessageId={cursor}
          openAt={view.openAt}
          focusMessageId={focusMessageId}
          start={<ConversationStart conversation={conversation} />}
          announce
          onAtBottomChange={setAtBottom}
          onJumpToLatest={toLatest}
          onJumpToFirstUnread={typeof cursor === 'string' ? () => setView({ around: cursor, openAt: 'unread', turn: view.turn }) : undefined}
          onMarkUnread={() => setSuspended(true)}
          onReply={canWrite ? setReplyTo : undefined}
          onFocusComposer={() => composer.current?.focus()}
        />
      )}
      {canWrite ? (
        <>
          <TypingLine conversationId={conversationId} />
          <ConnectionLine />
          <Composer
            ref={composer}
            conversation={conversation}
            replyTo={replyTo}
            onClearReply={() => setReplyTo(null)}
            autoFocus
            onEditLast={() => list.current?.editLastOwn() ?? false}
            onFocusList={() => list.current?.focusLast() ?? false}
            onSent={() => {
              // In a window around an old message the new message would not show: go to the newest messages.
              if (query.hasNextPage) toLatest()
            }}
          />
        </>
      ) : conversation.archived ? (
        <p data-slot="archived-notice" className="shrink-0 border-t border-border px-4 py-3 text-sm text-muted-foreground max-[899px]:px-3">
          This channel is archived. You can read it, but you cannot send messages.
        </p>
      ) : (
        <JoinBar conversation={conversation} title={title} />
      )}
      {dragging ? (
        <div
          data-slot="drop-hint"
          className="pointer-events-none absolute inset-2 z-30 flex items-center justify-center rounded-lg border-2 border-dashed border-primary bg-background/80 text-sm font-medium"
        >
          Drop files to add them to your message
        </div>
      ) : null}
    </section>
  )
}

/**
 * The message column below the header: the list, the typing and connection lines, and the composer; or the join bar
 * for a public channel the user is not in; or the archived notice. It owns the mark-read rule of the conversation.
 */
export function ConversationView({ conversationId, focusMessageId }: { conversationId: string; focusMessageId?: string | null }) {
  const conversation = useConversation(conversationId).data
  const state = useConversationState(conversationId)
  // The read cursor is captured when the column mounts, so it waits for the user's states. An unknown conversation is
  // the page's empty state, not this component's.
  if (!conversation || state.isPending) {
    return <section data-slot="conversation-view" aria-busy="true" className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background" />
  }
  return <ConversationColumn key={conversationId} conversation={conversation} state={state.data} focusMessageId={focusMessageId} />
}
