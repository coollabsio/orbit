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
import { defaultRangeExtractor, useVirtualizer, type Range } from '@tanstack/react-virtual'
import { ArrowDown, ArrowUp } from 'reicon-react'
import { toast } from 'sonner'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Spinner } from '@/components/ui/spinner'
import { wordlessPreview } from '@/lib/messagePreview'
import { useChatContext } from '../../api/chatContext'
import { useEditMessage, useSendMessage } from '../../api/mutations'
import type { Message } from '../../api/types'
import { canEdit } from '../../lib/forward'
import { buildMessageRows, messageKey, type MessageRow } from '../../lib/grouping'
import { decodeMentions } from '../../lib/mentionTokens'
import { dayChip, isSameDay } from '../../lib/time'
import { useDelayed, useIsPhone } from './environment'
import { ForwardDialog } from '../dialogs/ForwardDialog'
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
/** The space above the first message for the spinner of an older page. */
const OLDER_SPACE = 40
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
  /** "Reply" on a message: it becomes the composer's reply target. Absent where the user cannot write. */
  onReply?: (message: Message) => void
  /** Focus leaves the list for the composer (an edit that started there ends, or `↓` past the latest message). */
  onFocusComposer: () => void
}

type Overlay = { kind: 'menu' | 'picker' | 'sheet'; messageId: string; anchor: MenuAnchor; align: 'start' | 'end' }

/**
 * One item of the virtual list: the start of the history, the root of a thread, a message, the spinner at the end.
 * A day chip and the "New" line are part of the message that they come before: when a page comes or goes, the
 * virtualizer keeps the first item in view where it is, and finds it again by its key. Only a message has a key that
 * stays the same.
 */
type ListItem =
  | { type: 'head'; key: 'head' }
  | { type: 'root'; key: 'root' }
  | { type: 'tail'; key: 'tail' }
  | { type: 'message'; key: string; message: Message; groupStart: boolean; day?: number; isNew?: boolean }

/** The rows of `buildMessageRows` as list items. `skipDay` leaves a day's chip out (a thread shows it in its root). */
function messageItems(rows: readonly MessageRow[], skipDay: (at: number) => boolean): ListItem[] {
  const items: ListItem[] = []
  let day: number | undefined
  let isNew = false
  for (const row of rows) {
    if (row.type === 'day') day = skipDay(row.at) ? undefined : row.at
    else if (row.type === 'new') isNew = true
    else {
      items.push({ ...row, day, isNew })
      day = undefined
      isNew = false
    }
  }
  return items
}

/** A first guess of an item's height, until it is measured. A close guess keeps the scroll bar steady. */
function estimateSize(item: ListItem): number {
  switch (item.type) {
    case 'head':
      return 120
    case 'tail':
      return 40
    case 'root':
      return 96
    case 'message':
      return (
        (item.message.kind !== 'message' ? 24 : item.groupStart ? 56 : 26) +
        (item.message.replyToId ? 20 : 0) +
        (item.message.forwarded ? 40 : 0) +
        // a sticker is a 160px square in place of the line of text, or under it
        (item.message.sticker ? (item.message.body ? 164 : 140) : 0) +
        (item.day === undefined ? 0 : 44) +
        (item.isNew ? 24 : 0)
      )
  }
}

/**
 * A scrolling list of messages: day chips, the "New" line, grouped rows, paging in both directions with the scroll
 * position kept, the jump buttons, roving keyboard focus, and the one menu, emoji picker and action sheet that all
 * rows share. It does not fetch: `ConversationView` and `ThreadView` give it the messages.
 *
 * The list is virtual (TanStack Virtual, anchored to its end): only the rows in view and a few around them are in the
 * DOM. The virtualizer keeps the row in view where it is when pages come and go, and keeps the end in view while the
 * list is there. A row that is not in the DOM has no element: go to it with `scrollToIndex` first.
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
  onReply,
  onFocusComposer,
}: MessageListProps) {
  // The virtualizer is one object that changes inside: the React Compiler must not keep what is read from it.
  'use no memo'
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const phone = useIsPhone()
  const { retry, discard } = useSendMessage()
  const { mutate: editMessage } = useEditMessage()
  const showSpinner = useDelayed(loading, 300)

  const scroller = useRef<HTMLDivElement>(null)
  /** Scroll bookkeeping between renders; only effects and event handlers touch it. */
  const track = useRef({
    positioned: false,
    atBottom: false,
    fromBottom: 0,
    lastKey: null as string | null,
    jumpedTo: null as string | null,
    editFromComposer: false,
  })
  const [atBottom, setAtBottom] = useState(false)
  const [newCount, setNewCount] = useState(0)
  // The space above a list that is shorter than the pane, so a conversation's messages are at the bottom.
  const [fill, setFill] = useState(0)
  // The list follows new messages only from the render after the newest page came: the page itself must not move it.
  const [atTail, setAtTail] = useState(!hasNewer)
  const [announcement, setAnnouncement] = useState('')
  const [highlight, setHighlight] = useState<{ id: string; phase: 'highlight' | 'fading' } | null>(null)
  const [activeId, setActiveId] = useState<string | null>(null)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [overlay, setOverlay] = useState<Overlay | null>(null)
  const [overlayOpen, setOverlayOpen] = useState(false)
  // The message in the forward dialog. It stays while the dialog closes, so its preview does not go blank.
  const [forwarding, setForwarding] = useState<Message | null>(null)
  const [forwardOpen, setForwardOpen] = useState(false)

  const allMessages = root ? [root, ...messages] : messages
  // The first unread message is older than the loaded window: a "New" line at the top of the window would be wrong.
  const unreadBeforeWindow =
    typeof lastReadMessageId === 'string' && hasOlder && messages.length > 0 && messages[0].id > lastReadMessageId
  const rows = buildMessageRows(messages, {
    lastReadMessageId: unreadBeforeWindow ? undefined : lastReadMessageId,
    currentUserId: currentUserId ?? undefined,
  })
  const items: ListItem[] = [
    // The start of the history. The spinner of an older page is not an item: the virtualizer keeps the first item in
    // view where it is, and that must be a message, not a row that is the first one before and after the page.
    ...(!hasOlder && start ? [{ type: 'head', key: 'head' } as const] : []),
    // The root of a thread is the start of its history too: it shows when the first replies are loaded.
    ...(root && !hasOlder ? [{ type: 'root', key: 'root' } as const] : []),
    // The day of a thread's root is in the root's own time.
    ...messageItems(rows, (at) => root !== undefined && isSameDay(root.createdAt, at)),
    ...(loadingNewer ? [{ type: 'tail', key: 'tail' } as const] : []),
  ]
  const newIndex = items.findIndex((item) => item.type === 'message' && item.isNew)
  const indexOfMessage = (messageId: string | null | undefined) =>
    messageId ? items.findIndex((item) => (item.type === 'message' ? item.message.id === messageId : item.type === 'root' && root?.id === messageId)) : -1
  const latest = allMessages[allMessages.length - 1]
  const activeRowId = activeId !== null && allMessages.some((message) => message.id === activeId) ? activeId : latest?.id
  // Rows that must stay in the DOM out of view: the one that Tab goes to, an open edit, the anchor of the menu.
  const kept = [indexOfMessage(activeRowId), indexOfMessage(editingId), indexOfMessage(overlay?.messageId)].filter((index) => index !== -1)

  const virtualizer = useVirtualizer({
    count: items.length,
    enabled: !loading,
    getScrollElement: () => scroller.current,
    getItemKey: (index) => items[index].key,
    estimateSize: (index) => estimateSize(items[index]),
    anchorTo: 'end',
    followOnAppend: atTail && !hasNewer,
    scrollEndThreshold: BOTTOM_SLACK,
    overscan: 8,
    paddingStart: fill + (hasOlder ? OLDER_SPACE : 0),
    paddingEnd: 8,
    // A row that is scrolled to stays clear of the day chip.
    scrollPaddingStart: 48,
    rangeExtractor: (range: Range) => {
      const indexes = defaultRangeExtractor(range)
      const outside = kept.filter((index) => index < indexes[0] || index > indexes[indexes.length - 1])
      return outside.length > 0 ? [...new Set([...indexes, ...outside])].sort((a, b) => a - b) : indexes
    },
  })
  // An item above the view that changes its height (a day chip that goes when an older page comes) must not move
  // what the user sees. The default does not correct that while the list scrolls up.
  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (item, _delta, instance) => item.start < (instance.scrollOffset ?? 0)
  const firstVisible = virtualizer.range?.startIndex ?? 0
  // The "New" line is above the view, or (not loaded) before the window.
  const unreadAbove = track.current.positioned && (newIndex !== -1 ? newIndex < firstVisible : unreadBeforeWindow)
  // The day of the first row in view, for the chip that stays at the top of a list that is scrolled.
  const dayAbove = items.slice(0, firstVisible + 1).findLast((item) => item.type === 'message' && item.day !== undefined)

  const quickEmojis = quickReactions(allMessages, currentUserId)
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

  // The composer takes focus a frame later: a menu that closes puts focus on the message row first.
  const reply = onReply
    ? (message: Message) => {
        onReply(message)
        requestAnimationFrame(onFocusComposer)
      }
    : undefined

  function forward(message: Message) {
    setForwarding(message)
    setForwardOpen(true)
  }

  const { actionsFor, react, openThread, openQuoted, openOrigin } = useMessageActions({
    inThread,
    onReply: reply,
    onForward: forward,
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
    reply,
    openQuoted,
    openOrigin,
    forward,
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

  /** Reads the scroll position: at the bottom or not, and pages to load. */
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
    if (element.scrollTop < LOAD_AHEAD && hasOlder && !loadingOlder) onLoadOlder()
    if (fromBottom < LOAD_AHEAD && hasNewer && !loadingNewer) onLoadNewer()
  }

  function jumpToFirstUnread() {
    if (newIndex !== -1) virtualizer.scrollToIndex(newIndex, { align: 'start' })
    else onJumpToFirstUnread?.()
  }

  /** Focus on a message row. A row that is not in the DOM is scrolled to first. */
  function focusRow(messageId: string | undefined) {
    if (!messageId) return
    const row = rowElement(messageId)
    if (row) return row.focus()
    virtualizer.scrollToIndex(indexOfMessage(messageId), { align: 'auto' })
    let tries = 0
    const attempt = () => {
      const element = rowElement(messageId)
      if (element) element.focus()
      else if (tries++ < 10) requestAnimationFrame(attempt)
    }
    requestAnimationFrame(attempt)
  }

  useEffect(() => setAtTail(!hasNewer), [hasNewer])

  // A conversation with a few messages has them at the bottom of the pane.
  const viewport = virtualizer.scrollRect?.height ?? 0
  const wantedFill = inThread || loading ? 0 : Math.max(0, Math.round(viewport - (virtualizer.getTotalSize() - fill)))
  useLayoutEffect(() => {
    if (wantedFill !== fill) setFill(wantedFill)
  }, [wantedFill, fill])

  // An unmount ends what the virtualizer was scrolling to. In development React mounts every component twice
  // (StrictMode), so the list must take its first position again after that.
  useLayoutEffect(
    () => () => {
      track.current.positioned = false
      track.current.jumpedTo = null
    },
    [],
  )

  // After every change of the messages: place the list the first time, then keep what the user sees where it is.
  useLayoutEffect(() => {
    const element = scroller.current
    if (!element || loading) return
    const state = track.current
    const last = messages[messages.length - 1]
    const lastKey = last ? messageKey(last) : null

    if (!state.positioned) {
      state.positioned = true
      if (initialFromBottom !== undefined) {
        virtualizer.scrollToOffset(Math.max(0, virtualizer.getTotalSize() - element.clientHeight - initialFromBottom))
      } else if (openAt === 'unread' && newIndex !== -1) {
        virtualizer.scrollToIndex(newIndex, { align: 'start' })
      } else {
        virtualizer.scrollToEnd()
      }
    } else if (lastKey !== state.lastKey && !hasNewer) {
      const arrived = messagesAfter(messages, state.lastKey, messageKey, currentUserId)
      const sentNow = last?.authorId === currentUserId && last.sendState === 'sending'
      // The virtualizer follows new messages while the list is at the bottom; the user's own message always brings
      // it there.
      if (sentNow) virtualizer.scrollToEnd()
      else if (!state.atBottom && arrived.length > 0) setNewCount((count) => count + arrived.length)
      const newest = arrived[arrived.length - 1]
      if (announce && newest) {
        const text = firstLine(decodeMentions(newest.body, people.members, people.channels), 200)
        setAnnouncement(`${people.byId.get(newest.authorId)?.name ?? 'Someone'}: ${text || wordlessPreview(Boolean(newest.stickerId)).toLowerCase()}`)
      }
    }
    state.lastKey = lastKey
    // A frame later: the virtualizer moves the scroll position for a new page after this effect. Before that the
    // list can look as if it is at its top, and would ask for one more page with each page that comes.
    const frame = requestAnimationFrame(measure)
    return () => cancelAnimationFrame(frame)
    // `measure` and the callbacks read the current props; the effect itself only follows the messages.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [messages, loading, hasOlder])

  // A jump to a message (`?m=`): center it and highlight it, once it is loaded.
  useLayoutEffect(() => {
    const element = scroller.current
    const state = track.current
    if (!element || loading || !focusMessageId || state.jumpedTo === focusMessageId) return
    const index = indexOfMessage(focusMessageId)
    if (index === -1) return
    state.jumpedTo = focusMessageId
    virtualizer.scrollToIndex(index, { align: 'center' })
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

  // At the bottom the list stays there when the pane's height changes: the composer grows, the window resizes.
  useEffect(() => {
    const element = scroller.current
    if (!element || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => {
      if (track.current.atBottom) virtualizer.scrollToEnd()
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [virtualizer])

  /** The message rows that the arrow keys go through, in order (system rows are not among them). */
  const rowIds = items.flatMap((item) =>
    item.type === 'root' && root ? [root.id] : item.type === 'message' && item.message.kind === 'message' ? [item.message.id] : [],
  )

  useImperativeHandle(ref, () => ({
    editLastOwn: () => {
      const own = messages.findLast((message) => canEdit(message, currentUserId))
      if (!own) return false
      startEdit(own, true)
      return true
    },
    focusLast: () => {
      focusRow(rowIds[rowIds.length - 1])
      return rowIds.length > 0
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
    const index = rowIds.indexOf(row.dataset.messageId ?? '')
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
        focusRow(rowIds[index - 1])
        break
      case 'ArrowDown':
        if (index === rowIds.length - 1) onFocusComposer()
        else focusRow(rowIds[index + 1])
        break
      case 'Home':
        focusRow(rowIds[0])
        break
      case 'End':
        focusRow(rowIds[rowIds.length - 1])
        break
      case 'e':
      case 'E':
        if (!message || !canEdit(message, currentUserId)) return
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
            onClick={hasNewer ? onJumpToLatest : () => virtualizer.scrollToEnd()}
          >
            <ArrowDown />
            {hasNewer ? 'Jump to latest' : newCount === 1 ? '1 new message' : `${newCount} new messages`}
          </Button>
        ) : null}
        {dayAbove?.type === 'message' && dayAbove.day !== undefined && (virtualizer.scrollOffset ?? 0) > 0 ? (
          <div data-slot="day-chip-floating" className="pointer-events-none absolute inset-x-0 top-2 z-[5] flex justify-center py-2">
            <span className="rounded-full border border-border bg-background px-3 py-0.5 text-xs font-medium text-muted-foreground">{dayChip(dayAbove.day)}</span>
          </div>
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
            <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
              {hasOlder && loadingOlder ? (
                // The space is always there, so the spinner does not push the messages down when it shows.
                <div className="absolute inset-x-0 flex h-10 items-center justify-center" style={{ top: fill }}>
                  <Spinner className="text-muted-foreground" />
                </div>
              ) : null}
              {virtualizer.getVirtualItems().map((virtual) => {
                const item = items[virtual.index]
                return (
                  // `flow-root` keeps the margins of a row inside its measured box.
                  <div key={virtual.key} ref={virtualizer.measureElement} data-index={virtual.index} className="absolute left-0 flow-root w-full" style={{ top: virtual.start }}>
                    {item.type === 'head' ? (
                      start
                    ) : item.type === 'root' && root ? (
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
                    ) : item.type === 'tail' ? (
                      <div className="flex h-10 items-center justify-center">
                        <Spinner className="text-muted-foreground" />
                      </div>
                    ) : item.type === 'message' ? (
                      <>
                        {item.day === undefined ? null : <DayChip at={item.day} />}
                        {item.isNew ? <NewLine /> : null}
                        {item.message.kind !== 'message' ? (
                          <SystemRow message={item.message} />
                        ) : (
                          <MessageItem
                            message={item.message}
                            groupStart={item.groupStart}
                            editing={editingId === item.message.id}
                            active={activeRowId === item.message.id}
                            tone={highlight?.id === item.message.id ? highlight.phase : undefined}
                            menuOpen={overlayOpen && overlay?.kind !== 'sheet' && overlay?.messageId === item.message.id}
                          />
                        )}
                      </>
                    ) : null}
                  </div>
                )
              })}
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
              custom
              onPick={(emoji) => {
                if (overlayMessage) react(overlayMessage, emoji)
                onOverlayOpenChange(false)
              }}
            />
          </PopoverContent>
        </Popover>
      ) : null}
      {forwarding ? (
        <ForwardDialog
          message={forwarding}
          open={forwardOpen}
          onOpenChange={setForwardOpen}
          // Focus goes back to the message, not to the menu item or toolbar button that is gone by then.
          finalFocus={() => rowElement(forwarding.id) ?? true}
        />
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
