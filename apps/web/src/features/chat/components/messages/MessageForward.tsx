import { Forward } from 'reicon-react'
import { shortDate } from '@/lib/format'
import { useConversations } from '../../api/queries'
import type { ForwardOrigin, Message } from '../../api/types'
import { fullTimestamp } from '../../lib/time'
import { MessageBody } from './MessageBody'
import { useMessageList } from './messageListContext'
import { conversationTitle } from './people'

/**
 * What a forward says: "Forwarded", the copied message behind a quote bar, and under it who wrote the original and
 * when. When the reader can see the conversation it came from, its name and the date are a link that jumps to the
 * original message.
 */
export function MessageForward({ message, origin, active }: { message: Message; origin: ForwardOrigin; active: boolean }) {
  const { currentUserId, people, openOrigin } = useMessageList()
  const conversation = useConversations().data?.find((item) => item.id === origin.conversationId)
  const date = (
    <time dateTime={new Date(origin.createdAt).toISOString()} title={fullTimestamp(origin.createdAt)}>
      {shortDate(new Date(origin.createdAt).toISOString())}
    </time>
  )

  return (
    <div data-slot="message-forward" className="min-w-0">
      <div className="flex items-center gap-1 text-xs text-muted-foreground italic">
        <Forward aria-hidden="true" className="size-3.5" />
        Forwarded
      </div>
      <div className="border-l-4 border-muted-foreground/40 pl-3">
        <MessageBody message={message} />
      </div>
      <div data-slot="message-forward-origin" className="mt-1 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
        <span className="truncate">{people.byId.get(origin.authorId)?.name ?? 'Unknown'}</span>
        {conversation ? (
          <button
            type="button"
            // `Tab` reaches one message row, so only that row's link is a tab stop.
            tabIndex={active ? 0 : -1}
            className="flex min-w-0 items-center gap-1.5 rounded-sm outline-none hover:text-foreground hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
            onClick={() => openOrigin(message)}
          >
            <span className="truncate">{conversationTitle(conversation, people, currentUserId)}</span>
            <span aria-hidden="true">•</span>
            <span className="shrink-0">{date}</span>
          </button>
        ) : (
          <span className="shrink-0">{date}</span>
        )}
      </div>
    </div>
  )
}
