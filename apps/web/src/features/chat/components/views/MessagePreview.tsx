import { cn } from 'cn'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { Message } from '@/features/chat/api/types'
import { MessageBody } from '@/features/chat/components/messages/MessageBody'
import { fullTimestamp, messageTime } from '@/features/chat/lib/time'
import type { User } from '@/features/workspaces/models'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

/**
 * A message as the Unreads and Threads views show it: avatar, author, time and the rendered body. `clamp` keeps it to
 * two lines and takes it out of the pointer's way, for a row that is one link as a whole.
 */
export function MessagePreview({ message, people, clamp = false, className }: { message: Message; people: readonly User[]; clamp?: boolean; className?: string }) {
  const author = people.find((person) => person.id === message.authorId)
  return (
    <div data-slot="message-preview" data-clamp={clamp || undefined} className={cn('flex min-w-0 gap-2.5 data-clamp:pointer-events-none', className)}>
      {/* a clamped preview is one link as a whole: no profile there */}
      <ProfileTrigger userId={clamp ? null : author?.id} name={author?.name ?? 'Unknown'} kind="avatar" tabIndex={-1} className="mt-0.5 flex shrink-0 self-start max-[899px]:mt-0">
        <UserAvatar user={author} name="?" size={28} />
      </ProfileTrigger>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <ProfileTrigger userId={clamp ? null : author?.id} name={author?.name ?? 'Unknown'} className="truncate text-[13px] font-semibold text-foreground">
            {author?.name ?? 'Unknown'}
          </ProfileTrigger>
          <time className="shrink-0 text-xs text-muted-foreground" dateTime={new Date(message.createdAt).toISOString()} title={fullTimestamp(message.createdAt)}>
            {messageTime(message.createdAt)}
          </time>
        </div>
        {message.deleted ? (
          <p className="text-[13px] text-muted-foreground italic">This message was deleted</p>
        ) : (
          <div className="min-w-0 text-[13px] in-data-clamp:max-h-10 in-data-clamp:overflow-hidden max-[899px]:text-sm">
            <MessageBody message={message} />
          </div>
        )}
      </div>
    </div>
  )
}
