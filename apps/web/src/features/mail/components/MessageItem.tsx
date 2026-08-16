import { Avatar } from '../../../components/ui/Avatar'
import { fullDate, timeOfDay } from '../../../lib/format'
import type { MailMessage } from '../../../mock/types'
import { firstLine } from '../mailLib'

interface MessageItemProps {
  message: MailMessage
  expanded: boolean
  onToggle: () => void
}

export function MessageItem({ message, expanded, onToggle }: MessageItemProps) {
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
        <Avatar user={null} name={message.from.name} size={32} />
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
        <div className="mail-message-body">{message.body}</div>
      ) : (
        <div className="mail-message-preview truncate">{firstLine(message.body)}</div>
      )}
    </div>
  )
}
