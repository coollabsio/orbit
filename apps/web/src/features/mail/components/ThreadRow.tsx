import { useState } from 'react'
import { Paperclip, Star } from 'reicon-react'
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
      className="list-row mail-row"
      data-active={active ? 'true' : undefined}
      data-unread={thread.unread ? 'true' : undefined}
      data-dragging={dragging || undefined}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/mail-thread-id', thread.id)
        // drag image: a small pill with the subject, not the whole translucent row
        const ghost = document.createElement('div')
        ghost.className = 'mail-drag-ghost'
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
      <span className="mail-row-dot">{thread.unread ? <span className="unread-dot" /> : null}</span>
      <UserAvatar user={null} name={sender.name} size={32} />
      <div className="mail-row-main">
        <div className="mail-row-line1">
          <span className="mail-row-sender truncate">{sender.name}</span>
          <span className="spacer" />
          <span className="mail-row-time">{relativeTime(thread.updatedAt)}</span>
        </div>
        <div className="mail-row-subject truncate">{thread.subject}</div>
        <div className="mail-row-snippet truncate">{thread.snippet}</div>
      </div>
      <div className="mail-row-trailing" onClick={(e) => e.stopPropagation()}>
        <button
          className="icon-button mail-star"
          aria-label={thread.starred ? 'Unstar' : 'Star'}
          onClick={() => toggleThreadStar(thread.id)}
        >
          <Star
            size={16}
            weight={thread.starred ? 'Filled' : 'Outline'}
            color={thread.starred ? 'var(--warning-dot)' : undefined}
          />
        </button>
        {thread.hasAttachment ? <Paperclip size={14} /> : null}
      </div>
    </div>
  )
}
