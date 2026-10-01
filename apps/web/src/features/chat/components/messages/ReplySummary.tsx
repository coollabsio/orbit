import { UserAvatar } from '@/components/common/UserAvatar'
import { extractPreview } from '@/lib/messagePreview'
import { useThreadState } from '../../api/queries'
import type { Message } from '../../api/types'
import { decodeMentions } from '../../lib/mentionTokens'
import { relativeAgo } from '../../lib/time'
import { useMessageList } from './messageListContext'
import { replyCountLabel } from './messageText'

/**
 * The thread under a message that has replies: the newest reply on one line, then how many replies there are and when
 * the last one came. One reply is enough to tell if the thread is worth opening; more would repeat the thread in the
 * conversation. For a thread the user follows with unread replies the count is bold and carries the pink square (one
 * of the three places the square shows).
 */
export function ReplySummary({ message }: { message: Message }) {
  const { people, openThread } = useMessageList()
  const state = useThreadState(message.id).data
  const unread = Boolean(state?.following && state.unreadReplies > 0)
  const reply = message.lastReply
  const author = reply ? people.byId.get(reply.authorId) : undefined
  const text = reply ? extractPreview(decodeMentions(reply.body, people.members, people.channels)) : ''

  return (
    <button
      type="button"
      data-slot="reply-summary"
      data-unread={unread ? '' : undefined}
      className="mt-1.5 -ml-1 flex max-w-full rounded-md py-1 pr-2 pl-1 text-left text-xs text-muted-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring/50 hover-fine:hover:bg-muted"
      onClick={() => openThread(message)}
    >
      <span className="flex min-w-0 flex-col gap-1 border-l-2 border-border pl-2.5">
        {reply ? (
          <span data-slot="reply-preview" className="flex min-w-0 items-center gap-1.5">
            <UserAvatar user={author} name="?" size={16} />
            <span className="shrink-0 font-medium text-foreground/90">{author?.name ?? 'Unknown'}</span>
            <span className="truncate text-[13px]">{text || 'Sent a file'}</span>
          </span>
        ) : null}
        <span className="flex min-w-0 items-center gap-1.5">
          {unread ? <span data-slot="unread-marker" aria-hidden="true" className="size-1.5 shrink-0 bg-primary" /> : null}
          <span className="shrink-0 font-medium text-foreground/90 in-data-unread:font-bold in-data-unread:text-foreground">{replyCountLabel(message.replyCount)}</span>
          {unread ? <span className="sr-only">, {state?.unreadReplies} unread</span> : null}
          {message.lastReplyAt ? <span className="truncate in-data-unread:font-semibold">last reply {relativeAgo(message.lastReplyAt)}</span> : null}
        </span>
      </span>
    </button>
  )
}
