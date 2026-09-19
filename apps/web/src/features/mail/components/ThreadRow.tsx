import { useState } from 'react'
import { Paperclip, Star } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '../../../components/ui/UserAvatar'
import { relativeTime } from '../../../lib/format'
import { toggleThreadStar } from '../../../mock/actions'
import type { MailThread } from '../../../mock/types'
import { threadSender } from '../mailLib'

interface ThreadRowProps {
  thread: MailThread
  active: boolean
  onOpen: (threadId: string) => void
}

export function ThreadRow({ thread, active, onOpen }: ThreadRowProps) {
  const sender = threadSender(thread)
  const [dragging, setDragging] = useState(false)
  return (
    <div
      className="group/row flex min-h-16 w-full min-w-0 cursor-pointer items-start gap-2 border-b border-border px-3 py-2.5 text-left transition-colors hover:bg-foreground/[0.02] data-[active]:bg-muted data-[dragging]:bg-muted data-[dragging]:text-foreground"
      data-active={active ? 'true' : undefined}
      data-unread={thread.unread ? 'true' : undefined}
      data-dragging={dragging || undefined}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/mail-thread-id', thread.id)
        // drag image: a small pill with the subject, not the whole translucent row
        const ghost = document.createElement('div')
        Object.assign(ghost.style, {
          position: 'fixed',
          top: '-100px',
          left: '-100px',
          zIndex: '100',
          maxWidth: '260px',
          overflow: 'hidden',
          border: '1px solid var(--border)',
          borderRadius: '8px',
          background: 'var(--card)',
          padding: '6px 10px',
          fontSize: '12px',
          fontWeight: '500',
          color: 'var(--foreground)',
          whiteSpace: 'nowrap',
          textOverflow: 'ellipsis',
          boxShadow: '0 12px 32px rgba(0, 0, 0, 0.16)',
        })
        ghost.textContent = `✉️ ${thread.subject}`
        document.body.appendChild(ghost)
        e.dataTransfer.setDragImage(ghost, 12, 12)
        setTimeout(() => ghost.remove(), 0)
        setDragging(true)
      }}
      onDragEnd={() => setDragging(false)}
      role="button"
      tabIndex={0}
      onClick={() => onOpen(thread.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target === e.currentTarget) onOpen(thread.id)
      }}
    >
      <span className="flex max-h-8 w-2 shrink-0 items-center self-stretch">
        {thread.unread ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
      </span>
      <UserAvatar user={null} name={sender.name} size={32} />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-baseline gap-2">
          <span className="min-w-0 truncate text-[13px] font-normal text-foreground group-data-[unread]/row:font-semibold">
            {sender.name}
          </span>
          <span className="flex-1" />
          <span className="shrink-0 text-xs text-muted-foreground/70">{relativeTime(thread.updatedAt)}</span>
        </div>
        <div className="truncate text-[13px] text-foreground group-data-[unread]/row:font-semibold">{thread.subject}</div>
        <div className="truncate text-xs text-muted-foreground/70">{thread.snippet}</div>
      </div>
      <div className="flex shrink-0 flex-col items-center gap-1 text-muted-foreground/70" onClick={(e) => e.stopPropagation()}>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="text-muted-foreground/70"
          aria-label={thread.starred ? 'Unstar' : 'Star'}
          onClick={() => toggleThreadStar(thread.id)}
        >
          <Star className={cn('size-4', thread.starred && 'fill-[#fcd452] text-[#fcd452]')} />
        </Button>
        {thread.hasAttachment ? <Paperclip className="size-3.5" /> : null}
      </div>
    </div>
  )
}
