import {
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
} from 'react'
import { ArrowDown, ArrowUp } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Spinner } from '@/components/ui/spinner'
import { useChatContext } from '../../api/chatContext'
import { useEditMessage, useSendMessage } from '../../api/mutations'
import type { Message } from '../../api/types'
import { buildMessageRows, messageKey } from '../../lib/grouping'
import { decodeMentions } from '../../lib/mentionTokens'
import { isSameDay } from '../../lib/time'
import { useDelayed, useIsPhone } from './environment'
import { MessageItem } from './MessageItem'
import { MessageListContext, type MessageListContextValue } from './messageListContext'
import { MessageActionSheet, MessageMenu } from './MessageMenu'
import { DayChip, NewLine, RepliesDivider, SystemRow } from './MessageRows'
import { firstLine, messagesAfter, quickReactions } from './messageText'
import { useChatPeople } from './people'
import { useMessageActions, type MenuAnchor } from './useMessageActions'

/** Closer to the end than this counts as "at the bottom". */
const BOTTOM_SLACK = 24
/** Closer to an edge than this loads the next page in that direction. */
const LOAD_AHEAD = 240
const HIGHLIGHT_HOLD = 400
const HIGHLIGHT_FADE = 1500

export interface MessageListHandle {
  /** `↑` in an empty composer: edits the user's last message. False when there is none. */
  editLastOwn: () => boolean
  /** `Shift+Tab` from the composer: focus goes to the latest message. False when the list has none. */
  focusLast: () => boolean
  /** How far the list is from its end, to hand the scroll position to another view of the same thread. */
  distanceFromBottom: () => number
}

interface MessageListProps {
  ref?: Ref<MessageListHandle>
  /** The accessible name of the list. */
  label: string
  /** Ascending. In a thread: the replies. */
  messages: Message[]
  /** In a thread: its root, shown first with the "N replies" divider. */
  root?: Message
  inThread?: boolean
  /** First load: blank for 300ms, then a quiet spinner. */
  loading: boolean
  hasOlder: boolean
  /** The loaded window is not the live tail (after a jump to an old message). */
  hasNewer: boolean
  loadingOlder: boolean
  loadingNewer: boolean
  onLoadOlder: () => void
  onLoadNewer: () => void
  /** The read cursor captured when the view opened. `undefined`: no "New" line. */
  lastReadMessageId?: string | null
  /** Where the list starts: at the "New" line when it has one, or at the bottom. */
  openAt: 'unread' | 'bottom'
  /** Start this far from the end instead (the thread moved between the pane and the full view). */
  initialFromBottom?: number
  /** Scroll to this message and highlight it. */
  focusMessageId?: string | null
  /** The start-of-history block, shown when there is nothing older. */
  start?: ReactNode
  /** Announce new messages of other members to screen readers. */
  announce?: boolean
  onAtBottomChange: (atBottom: boolean) => void
  /** The window is not the live tail: load the newest messages. */
  onJumpToLatest?: () => void
  /** The first unread message is older than the loaded window: load around it. */
  onJumpToFirstUnread?: () => void
  onMarkUnread?: (message: Message) => void
  /** Focus leaves the list for the composer (an edit that started there ends, or `↓` past the latest message). */
  onFocusComposer: () => void
}

type Overlay = { kind: 'menu' | 'picker' | 'sheet'; messageId: string; anchor: MenuAnchor; align: 'start' | 'end' }

/**
 * A scrolling list of messages: day chips, the "New" line, grouped rows, paging in both directions with the scroll
 * position kept, the jump buttons, roving keyboard focus, and the one menu, emoji picker and action sheet that all
 * rows share. It does not fetch: `ConversationView` and `ThreadView` give it the messages.
 */
export function MessageList({
  ref,
  label,
  messages,
  root,
  inThread = false,
  loading,
  hasOlder,
  hasNewer,
  loadingOlder,
  loadingNewer,
  onLoadOlder,
  onLoadNewer,
  lastReadMessageId,
  openAt,
  initialFromBottom,
  focusMessageId,
  start,
  announce = false,
  onAtBottomChange,
  onJumpToLatest,
  onJumpToFirstUnread,
  onMarkUnread,
  onFocusComposer,
}: MessageListProps) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const phone = useIsPhone()
  const { retry, discard } = useSendMessage()
  const { mutate: editMessage } = useEditMessage()
  const showSpinner = useDelayed(loading, 300)

  const scroller = useRef<HTMLDivElement>(null)
  const content = useRef<HTMLDivElement>(null)
  /** Scroll bookkeeping between renders; only effects and event handlers touch it. */
  const track = useRef({
    positioned: false,
    atBottom: false,
    fromBottom: 0,
    firstKey: null as string | null,
    lastKey: null as string | null,
    hasOlder: false,
    jumpedTo: null as string | null,
    editFromComposer: false,
  })
  const [atBottom, setAtBottom] = useState(false)
  const [newCount, setNewCount] = useState(0)
  const [unreadAbove, setUnreadAbove] = useState(false)
  const [announcement, setAnnouncement] = useState('')
  const [highlight, setHighlight] = useState<{ id: string; phase: 'highlight' | 'fading' } | null>(null)
  const [activeId, setActiveId] = useState<string | null>(null)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [overlay, setOverlay] = useState<Overlay | null>(null)
  const [overlayOpen, setOverlayOpen] = useState(false)

  const allMessages = root ? [root, ...messages] : messages
  // The first unread message is older than the loaded window: a "New" line at the top of the window would be wrong.
  const unreadBeforeWindow =
    typeof lastReadMessageId === 'string' && hasOlder && messages.length > 0 && messages[0].id > lastReadMessageId
  const rows = buildMessageRows(messages, {
    lastReadMessageId: unreadBeforeWindow ? undefined : lastReadMessageId,
    currentUserId: currentUserId ?? undefined,
  })
  const quickEmojis = quickReactions(allMessages, currentUserId)
  const latest = allMessages[allMessages.length - 1]
  const activeRowId = activeId !== null && allMessages.some((message) => message.id === activeId) ? activeId : latest?.id
  const overlayMessage = overlay ? allMessages.find((message) => message.id === overlay.messageId) : undefined

  function rowElement(messageId: string): HTMLElement | null {
    return scroller.current?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(messageId)}"]`) ?? null
  }

  function showOverlay(kind: Overlay['kind'], message: Message, anchor: MenuAnchor, align: 'start' | 'end') {
    setOverlay({ kind, messageId: message.id, anchor, align })
    setOverlayOpen(true)
  }

  function onOverlayOpenChange(open: boolean) {
    setOverlayOpen(open)
    // Focus goes back to the message; an action that needs it elsewhere (edit, the emoji picker) takes it from there.
    if (!open && overlay) rowElement(overlay.messageId)?.focus({ preventScroll: true })
  }

  function startEdit(message: Message, fromComposer = false) {
    track.current.editFromComposer = fromComposer
    setEditingId(message.id)
  }

  function endEdit() {
    const messageId = editingId
    setEditingId(null)
    if (track.current.editFromComposer) onFocusComposer()
    else if (messageId) rowElement(messageId)?.focus({ preventScroll: true })
  }

  const { actionsFor, react, openThread } = useMessageActions({
    inThread,
    onEdit: (message) => startEdit(message),
    onMarkUnread,
    onAddReaction: (message) => {
      // The picker takes the menu's place (or the row's, from the sheet). It opens a frame later, so the click that
      // closed the menu is not an outside click for it.
      const anchor = overlay?.kind === 'menu' ? overlay.anchor : (rowElement(message.id) ?? undefined)
      const align = overlay?.kind === 'menu' ? overlay.align : 'start'
      if (anchor) requestAnimationFrame(() => showOverlay('picker', message, anchor, align))
    },
  })

  const context: MessageListContextValue = {
    currentUserId,
    inThread,
    phone,
    people,
    quickEmojis,
    react,
    openThread,
    openMenu: (message, anchor, align) => {
      if (actionsFor(message).length > 0) showOverlay('menu', message, anchor, align)
    },
    openPicker: (message, anchor, align) => showOverlay('picker', message, anchor, align),
    openSheet: (message) => {
      const row = rowElement(message.id)
      if (row && actionsFor(message).length > 0) showOverlay('sheet', message, row, 'start')
    },
    saveEdit: (message, body) => {
      editMessage({ messageId: message.id, body }, { onError: () => toast.error('Could not save the edit. Try again.') })
      endEdit()
    },
    endEdit,
    retry,
    discard,
  }

  /** Reads the scroll position: at the bottom or not, the "New" line above the viewport or not, and pages to load. */
  function measure() {
    const element = scroller.current
    const state = track.current
    if (!element || !state.positioned) return
    const fromBottom = element.scrollHeight - element.scrollTop - element.clientHeight
    state.fromBottom = fromBottom
    const bottom = fromBottom <= BOTTOM_SLACK && !hasNewer
    if (bottom !== state.atBottom) {
      state.atBottom = bottom
      setAtBottom(bottom)
      onAtBottomChange(bottom)
    }
    if (bottom) setNewCount(0)
    const line = element.querySelector('[data-slot="new-line"]')
    setUnreadAbove(line ? line.getBoundingClientRect().bottom < element.getBoundingClientRect().top : unreadBeforeWindow)
    if (element.scrollTop < LOAD_AHEAD && hasOlder && !loadingOlder) onLoadOlder()
    if (fromBottom < LOAD_AHEAD && hasNewer && !loadingNewer) onLoadNewer()
  }

  function scrollToEnd() {
    const element = scroller.current
    if (element) element.scrollTop = element.scrollHeight
  }

  function jumpToFirstUnread() {
    const element = scroller.current
    const line = element?.querySelector<HTMLElement>('[data-slot="new-line"]')
    if (element && line) element.scrollTop = line.offsetTop - 48
    else onJumpToFirstUnread?.()
  }

  // After every change of the messages: place the list the first time, then keep what the user sees where it is.
  useLayoutEffect(() => {
    const element = scroller.current
    if (!element || loading) return
    const state = track.current
    const firstKey = messages.length > 0 ? messageKey(messages[0]) : null
    const last = messages[messages.length - 1]
    const lastKey = last ? messageKey(last) : null

    if (!state.positioned) {
      state.positioned = true
      const line = openAt === 'unread' ? element.querySelector<HTMLElement>('[data-slot="new-line"]') : null
      if (initialFromBottom !== undefined) element.scrollTop = element.scrollHeight - element.clientHeight - initialFromBottom
      else element.scrollTop = line ? line.offsetTop - 48 : element.scrollHeight
    } else {
      // An older page came in above (or the start of the history took the spinner's place): the distance to the end
      // is what stays the same.
      if ((state.firstKey !== null && firstKey !== state.firstKey) || hasOlder !== state.hasOlder) {
        element.scrollTop = element.scrollHeight - element.clientHeight - state.fromBottom
      }
      if (lastKey !== state.lastKey && !hasNewer) {
        const arrived = messagesAfter(messages, state.lastKey, messageKey, currentUserId)
        const sentNow = last?.authorId === currentUserId && last.sendState === 'sending'
        // The list follows new messages only at the bottom; the user's own message always brings it there.
        if (state.atBottom || sentNow) element.scrollTop = element.scrollHeight
        else if (arrived.length > 0) setNewCount((count) => count + arrived.length)
        const newest = arrived[arrived.length - 1]
        if (announce && newest) {
          const text = firstLine(decodeMentions(newest.body, people.members, people.channels), 200)
          setAnnouncement(`${people.byId.get(newest.authorId)?.name ?? 'Someone'}: ${text || 'sent a file'}`)
        }
      }
    }
    state.firstKey = firstKey
    state.lastKey = lastKey
    state.hasOlder = hasOlder
    measure()
    // `measure` and the callbacks read the current props; the effect itself only follows the messages.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [messages, loading, hasOlder])

  // A jump to a message (`?m=`): center it and highlight it, once it is loaded.
  useLayoutEffect(() => {
    const element = scroller.current
    const state = track.current
    if (!element || loading || !focusMessageId || state.jumpedTo === focusMessageId) return
    const row = rowElement(focusMessageId)
    if (!row) return
    state.jumpedTo = focusMessageId
    element.scrollTop = row.offsetTop - (element.clientHeight - row.offsetHeight) / 2
    // The highlight starts when the row is in the DOM and scrolled to, which only an effect can know.
    // oxlint-disable-next-line react/set-state-in-effect
    setHighlight({ id: focusMessageId, phase: 'highlight' })
    setActiveId(focusMessageId)
    measure()
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [focusMessageId, messages, loading])

  // The highlight holds for a moment, fades for 1.5s, then is gone.
  useEffect(() => {
    if (!highlight) return
    const { id, phase } = highlight
    const timer = setTimeout(
      () => setHighlight(phase === 'highlight' ? { id, phase: 'fading' } : null),
      phase === 'highlight' ? HIGHLIGHT_HOLD : HIGHLIGHT_FADE,
    )
    return () => clearTimeout(timer)
  }, [highlight])

  // At the bottom the list stays there when its height changes: an image loads, the composer grows, the window resizes.
  useEffect(() => {
    const element = scroller.current
    const inner = content.current
    if (!element || !inner || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => {
      if (track.current.atBottom) element.scrollTop = element.scrollHeight
    })
    observer.observe(element)
    observer.observe(inner)
    return () => observer.disconnect()
    // The content element exists once the first load is over.
  }, [loading])

  useImperativeHandle(ref, () => ({
    editLastOwn: () => {
      const own = messages.findLast(
        (message) => message.authorId === currentUserId && message.kind === 'message' && !message.deleted && !message.sendState,
      )
      if (!own) return false
      startEdit(own, true)
      return true
    },
    focusLast: () => {
      const all = scroller.current?.querySelectorAll<HTMLElement>('[data-message-row]')
      const row = all?.[all.length - 1]
      row?.focus()
      return Boolean(row)
    },
    distanceFromBottom: () => track.current.fromBottom,
  }))

  function onFocus(event: FocusEvent<HTMLDivElement>) {
    const row = (event.target as HTMLElement).closest<HTMLElement>('[data-message-row]')
    if (row?.dataset.messageId) setActiveId(row.dataset.messageId)
  }

  // Keys on a focused message row (not on a control inside it).
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const row = event.target as HTMLElement
    if (!row.matches('[data-message-row]') || event.altKey || event.ctrlKey || event.metaKey) return
    const all = Array.from(scroller.current?.querySelectorAll<HTMLElement>('[data-message-row]') ?? [])
    const index = all.indexOf(row)
    const message = allMessages.find((candidate) => candidate.id === row.dataset.messageId)
    const editable = message && !message.sendState && !message.deleted && message.kind === 'message'

    if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
      event.preventDefault()
      if (message) context.openMenu(message, row, 'start')
      return
    }
    if (event.shiftKey) return
    switch (event.key) {
      case 'ArrowUp':
        all[index - 1]?.focus()
        break
      case 'ArrowDown':
        if (index === all.length - 1) onFocusComposer()
        else all[index + 1]?.focus()
        break
      case 'Home':
        all[0]?.focus()
        break
      case 'End':
        all[all.length - 1]?.focus()
        break
      case 'e':
      case 'E':
        if (!editable || message.authorId !== currentUserId) return
        startEdit(message)
        break
      case 't':
      case 'T':
      case 'ArrowRight':
        if (inThread || !message || message.sendState) return
        openThread(message)
        break
      case 'r':
      case 'R':
        if (!editable) return
        showOverlay('picker', message, row, 'start')
        break
      default:
        return
    }
    event.preventDefault()
  }

  return (
    <MessageListContext value={context}>
      <div data-slot="message-list" className="relative flex min-h-0 flex-1 flex-col">
        {unreadAbove ? (
          <Button
            variant="outline"
            size="sm"
            data-slot="jump-to-unread"
            // Below the sticky day chip.
            className="absolute top-11 left-1/2 z-20 -translate-x-1/2 rounded-full shadow-sm"
            onClick={jumpToFirstUnread}
          >
            <ArrowUp />
            Jump to first unread
          </Button>
        ) : null}
        {hasNewer || (!atBottom && newCount > 0) ? (
          <Button
            size="sm"
            data-slot="jump-to-latest"
            className="absolute bottom-2 left-1/2 z-20 -translate-x-1/2 rounded-full shadow-sm"
            onClick={hasNewer ? onJumpToLatest : scrollToEnd}
          >
            <ArrowDown />
            {hasNewer ? 'Jump to latest' : newCount === 1 ? '1 new message' : `${newCount} new messages`}
          </Button>
        ) : null}
        <div
          ref={scroller}
          role="list"
          aria-label={label}
          aria-busy={loading || undefined}
          data-slot="message-scroller"
          className="relative min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain [overflow-anchor:none]"
          onScroll={measure}
          onFocus={onFocus}
          onKeyDown={onKeyDown}
        >
          {loading ? (
            showSpinner ? (
              <div className="flex h-full items-center justify-center">
                <Spinner className="text-muted-foreground" />
              </div>
            ) : null
          ) : (
            <div ref={content} className={cn('flex min-h-full flex-col pb-2', !inThread && 'justify-end')}>
              {hasOlder ? (
                // The space is always there, so the spinner does not push the messages down when it shows.
                <div className="flex h-10 shrink-0 items-center justify-center">{loadingOlder ? <Spinner className="text-muted-foreground" /> : null}</div>
              ) : (
                start
              )}
              {root ? (
                <>
                  <MessageItem
                    message={root}
                    groupStart
                    editing={editingId === root.id}
                    active={activeRowId === root.id}
                    tone={highlight?.id === root.id ? highlight.phase : undefined}
                    menuOpen={overlayOpen && overlay?.kind !== 'sheet' && overlay?.messageId === root.id}
                  />
                  {root.replyCount > 0 || messages.length > 0 ? <RepliesDivider count={Math.max(root.replyCount, messages.length)} /> : null}
                </>
              ) : null}
              {rows.map((row) =>
                row.type === 'day' ? (
                  root && isSameDay(root.createdAt, row.at) ? null : <DayChip key={row.key} at={row.at} />
                ) : row.type === 'new' ? (
                  <NewLine key={row.key} />
                ) : row.message.kind !== 'message' ? (
                  <SystemRow key={row.key} message={row.message} />
                ) : (
                  <MessageItem
                    key={row.key}
                    message={row.message}
                    groupStart={row.groupStart}
                    editing={editingId === row.message.id}
                    active={activeRowId === row.message.id}
                    tone={highlight?.id === row.message.id ? highlight.phase : undefined}
                    menuOpen={overlayOpen && overlay?.kind !== 'sheet' && overlay?.messageId === row.message.id}
                  />
                ),
              )}
              {loadingNewer ? (
                <div className="flex h-10 shrink-0 items-center justify-center">
                  <Spinner className="text-muted-foreground" />
                </div>
              ) : null}
            </div>
          )}
        </div>
        {announce ? (
          <div aria-live="polite" data-slot="message-announcer" className="sr-only">
            {announcement}
          </div>
        ) : null}
      </div>

      {overlay?.kind === 'menu' ? (
        <MessageMenu
          open={overlayOpen}
          onOpenChange={onOverlayOpenChange}
          anchor={overlay.anchor}
          align={overlay.align}
          actions={overlayMessage ? actionsFor(overlayMessage) : []}
        />
      ) : null}
      {overlay?.kind === 'picker' ? (
        <Popover open={overlayOpen} onOpenChange={onOverlayOpenChange} modal={false}>
          <PopoverContent anchor={overlay.anchor} side="bottom" align={overlay.align} className="w-auto gap-0 p-0">
            <EmojiPicker
              onPick={(emoji) => {
                if (overlayMessage) react(overlayMessage, emoji)
                onOverlayOpenChange(false)
              }}
            />
          </PopoverContent>
        </Popover>
      ) : null}
      <MessageActionSheet
        open={overlayOpen && overlay?.kind === 'sheet'}
        onOpenChange={onOverlayOpenChange}
        quickEmojis={quickEmojis}
        onReact={overlayMessage && !overlayMessage.deleted ? (emoji) => react(overlayMessage, emoji) : undefined}
        actions={overlay?.kind === 'sheet' && overlayMessage ? actionsFor(overlayMessage) : []}
      />
    </MessageListContext>
  )
}
