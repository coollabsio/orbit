import { Forward, Message as MessageIcon, MoreH, Reply, SmileCircle } from 'reicon-react'
import { Emoji } from '@/components/common/Emoji'
import { Button } from '@/components/ui/button'
import type { Message } from '../../api/types'
import { useMessageList } from './messageListContext'

/**
 * The hover toolbar: three quick reactions, the emoji picker, reply, reply in thread, forward, and `…`. It sits at the right edge of
 * the row, as in Discord, so it is in the same place for every message and does not cover the text. It shows on hover
 * (fine pointers), while the row or something in it has keyboard focus, and while its menu or picker is open; never
 * on a phone, where a long press opens the action sheet. No animation.
 */
export function MessageToolbar({ message }: { message: Message }) {
  const { inThread, quickEmojis, react, reply, openThread, forward, openMenu, openPicker } = useMessageList()
  return (
    <div
      role="toolbar"
      aria-label="Message actions"
      data-slot="message-toolbar"
      className="absolute -top-5 right-4 z-10 hidden items-center gap-0.5 rounded-lg border border-border bg-background p-0.5 shadow-sm group-focus-visible/message:flex group-has-[:focus-visible]/message:flex group-data-menu-open/message:flex hover-fine:group-hover/message:flex max-[899px]:hidden!"
    >
      {quickEmojis.map((emoji) => (
        <Button key={emoji} variant="ghost" size="icon" className="text-base" aria-label={`React with ${emoji}`} onClick={() => react(message, emoji)}>
          <Emoji value={emoji} />
        </Button>
      ))}
      <Button
        variant="ghost"
        size="icon"
        aria-label="Add reaction"
        title="Add reaction"
        aria-haspopup="dialog"
        onClick={(event) => openPicker(message, event.currentTarget, 'end')}
      >
        <SmileCircle className="size-5" />
      </Button>
      {reply ? (
        <Button variant="ghost" size="icon" aria-label="Reply" title="Reply" onClick={() => reply(message)}>
          <Reply className="size-5" />
        </Button>
      ) : null}
      {inThread ? null : (
        <Button variant="ghost" size="icon" aria-label="Reply in Thread" title="Reply in Thread" onClick={() => openThread(message)}>
          <MessageIcon className="size-5" />
        </Button>
      )}
      <Button variant="ghost" size="icon" aria-label="Forward" title="Forward" aria-haspopup="dialog" onClick={() => forward(message)}>
        <Forward className="size-5" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label="More actions"
        title="More actions"
        aria-haspopup="menu"
        onClick={(event) => openMenu(message, event.currentTarget, 'end')}
      >
        <MoreH className="size-5" />
      </Button>
    </div>
  )
}
