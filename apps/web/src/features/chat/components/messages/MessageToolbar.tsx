import { Message as MessageIcon, MoreH } from 'reicon-react'
import { Button } from '@/components/ui/button'
import type { Message } from '../../api/types'
import { useMessageList } from './messageListContext'

/**
 * The hover toolbar: three quick reactions, reply in thread, and `…`. It sits on the right edge of the text measure.
 * It shows on hover (fine pointers) and while the row or something in it has keyboard focus; never on a phone, where
 * a long press opens the action sheet. No animation.
 */
export function MessageToolbar({ message }: { message: Message }) {
  const { inThread, quickEmojis, react, openThread, openMenu } = useMessageList()
  return (
    <div
      role="toolbar"
      aria-label="Message actions"
      data-slot="message-toolbar"
      className="absolute -top-5 right-0 z-10 hidden items-center gap-0.5 rounded-lg border border-border bg-background p-0.5 shadow-sm group-focus-visible/message:flex group-has-[:focus-visible]/message:flex group-data-menu-open/message:flex hover-fine:group-hover/message:flex max-[899px]:hidden!"
    >
      {quickEmojis.map((emoji) => (
        <Button key={emoji} variant="ghost" size="icon" className="text-base" aria-label={`React with ${emoji}`} onClick={() => react(message, emoji)}>
          {emoji}
        </Button>
      ))}
      {inThread ? null : (
        <Button variant="ghost" size="icon" aria-label="Reply in thread" title="Reply in thread" onClick={() => openThread(message)}>
          <MessageIcon weight="Filled" className="size-5" />
        </Button>
      )}
      <Button
        variant="ghost"
        size="icon"
        aria-label="More actions"
        title="More actions"
        aria-haspopup="menu"
        onClick={(event) => openMenu(message, event.currentTarget, 'end')}
      >
        <MoreH weight="Filled" className="size-5" />
      </Button>
    </div>
  )
}
