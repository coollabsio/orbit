import { useRef, useState } from 'react'
import { Send2, X } from 'reicon-react'
import { markChannelRead, sendChatMessage } from '../../../mock/actions'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'

interface ComposerProps {
  state: AppState
  channel: Channel
  replyTo: ChatMessage | null
  onClearReply: () => void
}

export function Composer({ state, channel, replyTo, onClearReply }: ComposerProps) {
  const [value, setValue] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const replyName = replyTo
    ? replyTo.authorType === 'user'
      ? (state.users.find((u) => u.id === replyTo.authorId)?.name ?? 'Unknown')
      : (replyTo.externalAuthor?.name ?? replyTo.authorType)
    : null

  const autosize = () => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 124)}px`
  }

  const send = () => {
    const content = value.trim()
    if (!content) return
    sendChatMessage(channel.id, content, replyTo?.id ?? null)
    markChannelRead(channel.id)
    setValue('')
    onClearReply()
    const el = textareaRef.current
    if (el) el.style.height = 'auto'
  }

  return (
    <div className="chat-composer">
      {replyTo && replyName ? (
        <div className="chat-composer-replying">
          <span className="truncate">
            Replying to <strong>{replyName}</strong>
          </span>
          <span className="spacer" />
          <button
            type="button"
            className="icon-button"
            aria-label="Cancel reply"
            onClick={onClearReply}
          >
            <X size={16} />
          </button>
        </div>
      ) : null}
      <div className="chat-composer-row">
        <textarea
          ref={textareaRef}
          className="chat-composer-input"
          rows={1}
          placeholder={`Message #${channel.name}`}
          value={value}
          onChange={(e) => {
            setValue(e.target.value)
            autosize()
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
        />
        <button
          type="button"
          className="icon-button chat-send"
          aria-label="Send message"
          data-ready={value.trim() ? 'true' : undefined}
          disabled={!value.trim()}
          onClick={send}
        >
          <Send2 size={16} />
        </button>
      </div>
    </div>
  )
}
