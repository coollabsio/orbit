import { useState } from 'react'
import { sendReply } from '../../../mock/actions'

interface ReplyComposerProps {
  threadId: string
  replyToName: string
}

export function ReplyComposer({ threadId, replyToName }: ReplyComposerProps) {
  const [body, setBody] = useState('')
  const canSend = body.trim() !== ''

  const send = () => {
    if (!canSend) return
    sendReply(threadId, body.trim())
    setBody('')
  }

  return (
    <div className="mail-reply">
      <div className="mail-reply-label">Reply to {replyToName}</div>
      <textarea
        className="mail-reply-textarea"
        placeholder="Write a reply…"
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            e.preventDefault()
            send()
          }
        }}
      />
      <div className="mail-reply-footer">
        <button className="button button-primary" disabled={!canSend} onClick={send}>
          Send
        </button>
        <span className="text-xs text-faint">⌘⏎ to send</span>
      </div>
    </div>
  )
}
