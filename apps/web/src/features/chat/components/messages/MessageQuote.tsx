import { EmojiText } from '@/components/common/Emoji'
import { UserAvatar } from '@/components/common/UserAvatar'
import { extractPreview, wordlessPreview } from '@/lib/messagePreview'
import type { Message } from '../../api/types'
import { decodeMentions } from '../../lib/mentionTokens'
import { useMessageList } from './messageListContext'

/**
 * The line above an inline reply: the author of the message it answers and the start of that message, on one line of
 * plain text. A click jumps to the message. When the message was deleted the line says so and does nothing.
 */
export function MessageQuote({ message, active }: { message: Message; active: boolean }) {
  const { people, openQuoted } = useMessageList()
  const quote = message.replyTo ?? null
  const author = quote ? people.byId.get(quote.authorId) : undefined
  const text = quote ? extractPreview(decodeMentions(quote.body, people.members, people.channels)) : ''

  return (
    <div data-slot="message-quote" className="flex max-w-[calc(90ch+3rem)] items-center gap-3 text-xs text-muted-foreground max-[899px]:gap-2">
      {/* The spine: from the middle of this line down to the middle of the avatar. */}
      <div aria-hidden="true" className="flex w-9 shrink-0 justify-end self-end max-[899px]:w-[30px]">
        <span className="-mr-2 h-2.5 w-[26px] rounded-tl-md border-t-2 border-l-2 border-border max-[899px]:-mr-1 max-[899px]:w-[19px]" />
      </div>
      {quote ? (
        <button
          type="button"
          // `Tab` reaches one message row, so only that row's quote is a tab stop.
          tabIndex={active ? 0 : -1}
          className="flex h-5 min-w-0 items-center gap-1.5 rounded-sm text-left outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
          onClick={() => openQuoted(message)}
        >
          <UserAvatar user={author} name="?" size={16} />
          <span className="shrink-0 font-medium text-foreground/90">{author?.name ?? 'Unknown'}</span>
          <span className="truncate text-[13px]">
            <EmojiText text={text || wordlessPreview(quote.sticker === true)} />
          </span>
        </button>
      ) : (
        <span className="truncate leading-5 italic">Original message was deleted</span>
      )}
    </div>
  )
}
