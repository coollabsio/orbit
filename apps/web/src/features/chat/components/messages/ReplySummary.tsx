import { ProfileTrigger } from '@/components/common/ProfileTrigger'
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
    // The button that opens the thread covers the whole block (its `after`); the author of the reply sits above it.
    <div
      data-slot="reply-summary"
      data-unread={unread ? '' : undefined}
      className="relative mt-1.5 -ml-1 flex w-fit max-w-full rounded-md py-1 pr-2 pl-1 text-xs text-muted-foreground hover-fine:hover:bg-muted"
    >
      <span className="flex min-w-0 flex-col gap-1 border-l-2 border-border pl-2.5">
        {reply ? (
          <span data-slot="reply-preview" className="flex min-w-0 items-center gap-1.5">
            <ProfileTrigger userId={author?.id} name={author?.name ?? 'Unknown'} kind="avatar" tabIndex={-1} className="relative z-1 flex shrink-0">
              <UserAvatar user={author} name="?" size={16} />
            </ProfileTrigger>
            <ProfileTrigger userId={author?.id} name={author?.name ?? 'Unknown'} tabIndex={-1} className="relative z-1 shrink-0 font-medium text-foreground/90">
              {author?.name ?? 'Unknown'}
            </ProfileTrigger>
            <span className="truncate text-[13px]">{text || 'Sent a file'}</span>
          </span>
        ) : null}
        <button
          type="button"
          className="flex min-w-0 items-center gap-1.5 text-left outline-none after:absolute after:inset-0 after:rounded-md focus-visible:after:ring-3 focus-visible:after:ring-ring/50"
          onClick={() => openThread(message)}
        >
          {unread ? <span data-slot="unread-marker" aria-hidden="true" className="size-1.5 shrink-0 bg-primary" /> : null}
          <span className="shrink-0 font-medium text-foreground/90 in-data-unread:font-bold in-data-unread:text-foreground">{replyCountLabel(message.replyCount)}</span>
          {unread ? <span className="sr-only">, {state?.unreadReplies} unread</span> : null}
          {message.lastReplyAt ? <span className="truncate in-data-unread:font-semibold">last reply {relativeAgo(message.lastReplyAt)}</span> : null}
        </button>
      </span>
    </div>
  )
}
