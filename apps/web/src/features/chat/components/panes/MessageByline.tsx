import type { ReactNode } from 'react'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { Message } from '@/features/chat/api/types'
import { fullTimestamp, relativeAgo } from '@/features/chat/lib/time'
import type { User } from '@/features/workspaces/models'

interface MessageBylineProps {
  message: Pick<Message, 'authorId' | 'createdAt'>
  members: readonly User[]
  /** After the time, for example the conversation of a search result. */
  children?: ReactNode
}

/** Who wrote a message and when: small avatar, neutral name, muted time. Spans only, so it can sit inside a button. */
export function MessageByline({ message, members, children }: MessageBylineProps) {
  const author = members.find((member) => member.id === message.authorId)
  return (
    <span data-slot="message-byline" className="flex min-w-0 items-center gap-1.5 text-[13px]">
      <UserAvatar user={author} name="?" size={20} />
      <span className="truncate font-medium text-foreground">{author?.name ?? 'Unknown'}</span>
      <time dateTime={new Date(message.createdAt).toISOString()} title={fullTimestamp(message.createdAt)} className="shrink-0 text-xs font-normal text-muted-foreground">
        {relativeAgo(message.createdAt)}
      </time>
      {children}
    </span>
  )
}
