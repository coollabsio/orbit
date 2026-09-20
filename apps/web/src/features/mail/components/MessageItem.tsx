import { Forward, Reply } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '@/components/common/UserAvatar'
import { fullDate, timeOfDay } from '@/lib/format'
import type { MailMessage } from '@/mock/types'
import { firstLine } from '@/features/mail/mailLib'
import { Attachments } from '@/components/common/Attachments'

interface MessageItemProps {
  message: MailMessage
  expanded: boolean
  onToggle: () => void
  onReply: () => void
  onForward: () => void
}

export function MessageItem({ message, expanded, onToggle, onReply, onForward }: MessageItemProps) {
  return (
    <div
      data-slot="mail-message"
      className="py-4 [[data-slot=mail-message]+&]:border-t [[data-slot=mail-message]+&]:border-t-border"
      data-expanded={expanded ? 'true' : undefined}
    >
      <div
        className="-m-1 flex cursor-pointer items-center gap-2.5 rounded-md p-1 transition-colors hover:bg-muted/45"
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && e.target === e.currentTarget) onToggle()
        }}
      >
        <UserAvatar user={null} name={message.from.name} size={32} />
        <div className="flex min-w-0 flex-col">
          <span className="truncate text-[13px] font-semibold text-foreground">{message.from.name}</span>
          <span className="truncate text-xs text-muted-foreground/70">{message.from.email}</span>
        </div>
        <span className="flex-1" />
        <span className="shrink-0 text-xs text-muted-foreground/70">
          {fullDate(message.createdAt)}, {timeOfDay(message.createdAt)}
        </span>
      </div>
      {expanded ? (
        <>
          <div className="mt-3 text-sm leading-[1.6] whitespace-pre-wrap text-foreground">{message.body}</div>
          {message.attachments && message.attachments.length > 0 ? (
            <Attachments attachments={message.attachments} hasTextContent={!!message.body.trim()} />
          ) : null}
          <div className="mt-3 ml-[38px] flex gap-1">
            <Button type="button" variant="ghost" size="sm" className="text-xs text-muted-foreground hover:text-foreground" onClick={onReply}>
              <Reply className="size-3.5" />
              Reply
            </Button>
            <Button type="button" variant="ghost" size="sm" className="text-xs text-muted-foreground hover:text-foreground" onClick={onForward}>
              <Forward className="size-3.5" />
              Forward
            </Button>
          </div>
        </>
      ) : (
        <div className="mt-1.5 truncate pl-[42px] text-[13px] text-muted-foreground">{firstLine(message.body)}</div>
      )}
    </div>
  )
}
