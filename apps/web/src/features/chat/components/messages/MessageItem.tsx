import { useEffect, useRef, type MouseEvent, type TouchEvent } from 'react'
import { PinTack } from 'reicon-react'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import type { Message } from '../../api/types'
import { fullTimestamp, gutterTime, messageTime } from '../../lib/time'
import { isCoarsePointer, useDelayed } from './environment'
import { MessageBody } from './MessageBody'
import { MessageEditor } from './MessageEditor'
import { useMessageList } from './messageListContext'
import { MessageReactions } from './MessageReactions'
import { MessageToolbar } from './MessageToolbar'
import { ReplySummary } from './ReplySummary'
import { pointAnchor } from './useMessageActions'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

const LONG_PRESS = 500
/** A finger that moves this far is scrolling, not pressing. */
const PRESS_SLOP = 10

/** "Sending" shows only when the send takes a while (offline, reconnecting), so a normal send does not flash it. */
function SendingNote() {
  const slow = useDelayed(true, 1000)
  return <span className={slow ? 'text-xs text-muted-foreground' : 'sr-only'}>Sending</span>
}

function FailedNote({ message }: { message: Message }) {
  const { retry, discard } = useMessageList()
  return (
    <div role="alert" data-slot="message-failed" className="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-destructive">
      <span>Not sent.</span>
      <Button variant="link" size="xs" className="h-6 px-0 text-destructive" onClick={() => retry(message)}>
        Try again
      </Button>
      <Button variant="link" size="xs" className="h-6 px-0 text-muted-foreground" onClick={() => discard(message)}>
        Delete
      </Button>
    </div>
  )
}

interface MessageItemProps {
  message: Message
  /** First of a group: avatar, name and time. Others show the text only, with the time in the gutter. */
  groupStart: boolean
  editing: boolean
  /** The one row that `Tab` reaches; the arrow keys move between rows. */
  active: boolean
  /** A jump to this message: a background that then fades out. */
  tone?: 'highlight' | 'fading'
  /** Its menu is open: the toolbar stays. */
  menuOpen: boolean
}

/** One message row. Keyboard actions on the focused row (`E`, `T`, `R`, the Menu key) are handled by `MessageList`. */
export function MessageItem({ message, groupStart, editing, active, tone, menuOpen }: MessageItemProps) {
  const { currentUserId, inThread, phone, people, openThread, openMenu, openSheet } = useMessageList()
  const press = useRef<{ timer: ReturnType<typeof setTimeout>; x: number; y: number } | null>(null)
  const author = people.byId.get(message.authorId)
  const own = message.authorId === currentUserId
  const mentioned =
    !message.deleted &&
    !own &&
    ((currentUserId !== null && message.mentions.userIds.includes(currentUserId)) || message.mentions.channel || message.mentions.here)
  const confirmed = !message.sendState
  const iso = new Date(message.createdAt).toISOString()

  function cancelPress() {
    if (press.current) clearTimeout(press.current.timer)
    press.current = null
  }

  useEffect(() => cancelPress, [])

  // A long press opens the action sheet. Touch events only come from a touch screen, so a mouse never triggers it.
  function onTouchStart(event: TouchEvent<HTMLDivElement>) {
    cancelPress()
    if (!confirmed || editing || event.touches.length !== 1) return
    const { clientX: x, clientY: y } = event.touches[0]
    press.current = {
      x,
      y,
      timer: setTimeout(() => {
        press.current = null
        openSheet(message)
      }, LONG_PRESS),
    }
  }

  function onTouchMove(event: TouchEvent<HTMLDivElement>) {
    const start = press.current
    const touch = event.touches[0]
    if (start && touch && Math.hypot(touch.clientX - start.x, touch.clientY - start.y) > PRESS_SLOP) cancelPress()
  }

  function onContextMenu(event: MouseEvent<HTMLDivElement>) {
    const target = event.target as HTMLElement
    // Links, the edit field and selected text keep the browser's own menu.
    if (!confirmed || editing || target.closest('a, textarea, input') || window.getSelection()?.toString()) return
    event.preventDefault()
    // On a touch screen the same gesture is the long press, which opens the sheet.
    if (!phone && !isCoarsePointer()) openMenu(message, pointAnchor(event.clientX, event.clientY), 'start')
  }

  return (
    <div
      role="listitem"
      tabIndex={active ? 0 : -1}
      data-slot="message"
      data-message-row=""
      data-message-id={message.id}
      data-group-start={groupStart ? '' : undefined}
      data-mentioned={mentioned ? '' : undefined}
      data-tone={tone}
      data-state={message.sendState}
      data-menu-open={menuOpen ? '' : undefined}
      className="group/message relative px-4 py-0.5 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset data-group-start:mt-2 data-group-start:pt-1 data-mentioned:not-data-[tone=highlight]:bg-primary/8 data-[tone=fading]:transition-[background-color] data-[tone=fading]:duration-[1500ms] data-[tone=fading]:ease-out data-[tone=highlight]:bg-primary/15 hover-fine:not-data-mentioned:not-data-tone:hover:bg-muted/40 max-[899px]:px-3 max-[899px]:select-none max-[899px]:[-webkit-touch-callout:none]"
      onContextMenu={onContextMenu}
      onTouchStart={onTouchStart}
      onTouchMove={onTouchMove}
      onTouchEnd={cancelPress}
      onTouchCancel={cancelPress}
    >
      {confirmed && !editing && !message.deleted ? <MessageToolbar message={message} /> : null}
      {/* The measure: at most 90 characters of text beside the avatar column. */}
      <div className="relative flex max-w-[calc(90ch+3rem)] gap-3 text-[15px] max-[899px]:gap-2 max-[899px]:text-sm">
        <div className="flex w-9 shrink-0 justify-center max-[899px]:w-[30px]">
          {groupStart ? (
            <ProfileTrigger userId={author?.id} name={author?.name ?? 'Unknown'} kind="avatar" tabIndex={-1} className="mt-0.5 flex self-start">
              <UserAvatar user={author} name="?" size={phone ? 30 : 36} />
            </ProfileTrigger>
          ) : (
            <time
              dateTime={iso}
              title={fullTimestamp(message.createdAt)}
              className="hidden text-[11px] leading-[22.5px] text-muted-foreground max-[899px]:leading-[21px] tabular-nums group-focus-visible/message:block hover-fine:group-hover/message:block"
            >
              {gutterTime(message.createdAt)}
            </time>
          )}
        </div>
        <div className="min-w-0 flex-1">
          {groupStart ? (
            <div className="flex items-baseline gap-2">
              {/* `Tab` reaches one message row (the arrow keys move between rows), so only its name is a tab stop */}
              <ProfileTrigger userId={author?.id} name={author?.name ?? 'Unknown'} tabIndex={active ? 0 : -1} className="truncate font-medium text-foreground">
                <span data-slot="message-author">{author?.name ?? 'Unknown'}</span>
              </ProfileTrigger>
              <time dateTime={iso} title={fullTimestamp(message.createdAt)} className="shrink-0 text-xs text-muted-foreground">
                {messageTime(message.createdAt)}
              </time>
              {message.sendState === 'sending' ? <SendingNote /> : null}
            </div>
          ) : message.sendState === 'sending' ? (
            <span className="sr-only">Sending</span>
          ) : null}
          {message.threadRootId && !inThread ? (
            <button
              type="button"
              data-slot="thread-reply-line"
              className="flex h-6 items-center rounded-sm text-xs text-muted-foreground outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
              onClick={() => openThread(message)}
            >
              replied to a thread
            </button>
          ) : null}
          {message.pinned && !message.deleted ? (
            <div data-slot="message-pinned" className="flex items-center gap-1 text-xs text-muted-foreground">
              <PinTack className="size-3" />
              Pinned
            </div>
          ) : null}
          {editing ? (
            <MessageEditor message={message} />
          ) : (
            <div className="in-data-[state=failed]:text-muted-foreground in-data-[state=sending]:opacity-60">
              <MessageBody message={message} />
            </div>
          )}
          {message.sendState === 'failed' ? <FailedNote message={message} /> : null}
          {confirmed ? <MessageReactions message={message} /> : null}
          {!inThread && confirmed && message.threadRootId === null && message.replyCount > 0 ? <ReplySummary message={message} /> : null}
        </div>
      </div>
    </div>
  )
}
