// Port of the chat reference NewThreadPanel: title input above a composer; sending creates the
// thread-starter root plus the first reply, then opens the thread.
import { useRef, useState } from 'react'
import { Messages, Xmark } from 'reicon-react'
import { createThread } from '../../../mock/actions'
import type { AppState, Channel } from '../../../mock/types'
import { MessageInput } from './MessageInput'

export function NewThreadPanel({
  state,
  channel,
  onClose,
  onCreated,
}: {
  state: AppState
  channel: Channel
  onClose: () => void
  onCreated: (rootId: string) => void
}) {
  const [title, setTitle] = useState('')
  const titleRef = useRef<HTMLInputElement>(null)

  function handleSend(content: string) {
    const trimmedTitle = title.trim()
    if (!trimmedTitle) {
      titleRef.current?.focus()
      return false
    }
    onCreated(createThread(channel.id, trimmedTitle, content))
    return true
  }

  return (
    <div className="fc-thread-panel fc-new-thread">
      <div className="fc-thread-header">
        <div className="fc-thread-header-title">
          <Messages size={20} />
          <h2>New Thread</h2>
        </div>
        <button type="button" className="fc-thread-icon-button" title="Close new thread" aria-label="Close new thread" onClick={onClose}>
          <Xmark size={16} />
        </button>
      </div>
      <div className="spacer" />
      <div className="fc-new-thread-form">
        <div className="fc-new-thread-icon">
          <Messages size={28} />
        </div>
        <label className="fc-new-thread-label">
          <span>Thread Name</span>
          <input
            ref={titleRef}
            className="fc-new-thread-input"
            value={title}
            placeholder="New Thread"
            autoFocus
            onChange={(e) => setTitle(e.target.value)}
          />
        </label>
      </div>
      <MessageInput
        state={state}
        channel={channel}
        placeholder="Enter a message to start the conversation!"
        onSend={handleSend}
      />
    </div>
  )
}
