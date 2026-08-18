import { useEffect, useRef, useState } from 'react'
import { CloseCircle, EmojiHappy, Send2 } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import { sendChatMessage } from '../../../mock/actions'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { displayName } from '../chatLib'

const EMOJIS = [
  '😀', '😂', '😅', '😍', '🤔', '😴', '😭', '🥳',
  '👍', '👎', '👀', '👋', '🙏', '💪', '🤝', '👏',
  '❤️', '🔥', '🎉', '🚀', '⭐', '✅', '❌', '💡',
]

interface ComposerProps {
  state: AppState
  channel: Channel
  replyTo: ChatMessage | null
  onClearReply: () => void
}

export function Composer({ state, channel, replyTo, onClearReply }: ComposerProps) {
  const [text, setText] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (replyTo) textareaRef.current?.focus()
  }, [replyTo])

  const resize = () => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 320)}px`
  }

  const send = () => {
    const content = text.trim()
    if (!content) return
    sendChatMessage(channel.id, content, replyTo?.id ?? null)
    setText('')
    onClearReply()
    const el = textareaRef.current
    if (el) el.style.height = '24px'
  }

  const insertEmoji = (emoji: string) => {
    const el = textareaRef.current
    if (!el) {
      setText((t) => t + emoji)
      return
    }
    const start = el.selectionStart ?? text.length
    const end = el.selectionEnd ?? text.length
    setText(text.slice(0, start) + emoji + text.slice(end))
    requestAnimationFrame(() => {
      el.focus()
      el.setSelectionRange(start + emoji.length, start + emoji.length)
    })
  }

  return (
    <div className="chat-composer">
      {replyTo ? (
        <div className="chat-reply-bar">
          <span className="chat-reply-bar-label">Replying to {displayName(state, replyTo)}</span>
          <span className="chat-reply-bar-preview">{replyTo.content}</span>
          <button type="button" className="icon-button" aria-label="Cancel reply" onClick={onClearReply}>
            <CloseCircle size={14} />
          </button>
        </div>
      ) : null}
      <div className="chat-input-row" data-replying={replyTo ? 'true' : undefined}>
        <textarea
          ref={textareaRef}
          className="chat-input"
          rows={1}
          style={{ height: 24 }}
          placeholder={`Message #${channel.name}`}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onInput={resize}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
        />
        <Dropdown
          align="right"
          direction="up"
          trigger={() => (
            <span className="icon-button" aria-label="Emoji">
              <EmojiHappy size={18} />
            </span>
          )}
        >
          {(close) => (
            <div className="chat-emoji-grid">
              {EMOJIS.map((emoji) => (
                <button
                  key={emoji}
                  type="button"
                  onClick={() => {
                    insertEmoji(emoji)
                    close()
                  }}
                >
                  {emoji}
                </button>
              ))}
            </div>
          )}
        </Dropdown>
        <button
          type="button"
          className="icon-button"
          aria-label="Send"
          disabled={!text.trim()}
          onClick={send}
        >
          <Send2 size={18} />
        </button>
      </div>
    </div>
  )
}
