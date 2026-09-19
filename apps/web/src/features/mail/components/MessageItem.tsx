import { UserAvatar } from '../../../components/ui/UserAvatar'
import { fullDate, timeOfDay } from '../../../lib/format'
import type { MailMessage } from '../../../mock/types'
import { firstLine } from '../mailLib'
import { Attachments } from '../../chat/components/Attachments'

interface MessageItemProps {
  message: MailMessage
  expanded: boolean
  onToggle: () => void
  onReply: () => void
  onForward: () => void
}

export function MessageItem({ message, expanded, onToggle, onReply, onForward }: MessageItemProps) {
  return (
    <div className="mail-message" data-expanded={expanded ? 'true' : undefined}>
      <div
        className="mail-message-header"
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && e.target === e.currentTarget) onToggle()
        }}
      >
        <UserAvatar user={null} name={message.from.name} size={32} />
        <div className="mail-message-meta">
          <span className="mail-message-sender truncate">{message.from.name}</span>
          <span className="text-xs text-faint truncate">{message.from.email}</span>
        </div>
        <span className="spacer" />
        <span className="text-xs text-faint mail-message-date">
          {fullDate(message.createdAt)}, {timeOfDay(message.createdAt)}
        </span>
      </div>
      {expanded ? (
        <>
          <div className="mail-message-body">{message.body}</div>
          {message.attachments && message.attachments.length > 0 ? (
            <Attachments attachments={message.attachments} hasTextContent={!!message.body.trim()} />
          ) : null}
          <div className="mail-message-actions">
            <button type="button" className="button button-ghost" onClick={onReply}>
              <Reply size={15} />
              Reply
            </button>
            <button type="button" className="button button-ghost" onClick={onForward}>
              <Forward size={15} />
              Forward
            </button>
          </div>
        </>
      ) : (
        <div className="mail-message-preview truncate">{firstLine(message.body)}</div>
      )}
    </div>
  )
}
import { Forward, Reply } from 'reicon-react'
