// Chat area: h-12 header (# + name + topic, right actions: threads, members)
// over MessageList + TypingIndicator + MessageInput.
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { ArrowLeft, Hashtag, Messages, People } from 'reicon-react'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { MessageInput } from './MessageInput'
import { MessageList } from './MessageList'
import { ThreadsPopover } from './ThreadsPopover'
import { TypingIndicator } from './TypingIndicator'

export function ChatArea({
  state,
  channel,
  membersOpen,
  onToggleMembers,
  onOpenThread,
  onNewThread,
}: {
  state: AppState
  channel: Channel
  membersOpen: boolean
  onToggleMembers: () => void
  onOpenThread: (root: ChatMessage) => void
  onNewThread: () => void
}) {
  const navigate = useNavigate()
  const [replyTarget, setReplyTarget] = useState<ChatMessage | null>(null)
  const [threadsOpen, setThreadsOpen] = useState(false)

  return (
    <div className="fc-chat-area">
      <div className="fc-chat-header">
        <div className="fc-chat-header-left">
          <button type="button" className="fc-mobile-back" title="Back to channels" onClick={() => navigate('/chat')}>
            <ArrowLeft size={18} />
          </button>
          {channel.emoji ? <span className="fc-emoji-icon" data-size="lg">{channel.emoji}</span> : <Hashtag size={20} />}
          <div className="fc-chat-header-title">
            <h2>{channel.name}</h2>
            {channel.description ? (
              <div className="fc-chat-topic">
                <span>{channel.description}</span>
              </div>
            ) : null}
          </div>
        </div>
        <div className="fc-chat-header-actions">
          <button
            type="button"
            className="fc-header-button"
            data-active={threadsOpen ? 'true' : undefined}
            title="Threads"
            onClick={() => setThreadsOpen((o) => !o)}
          >
            <Messages size={20} />
          </button>
          <button
            type="button"
            className="fc-header-button"
            data-active={membersOpen ? 'true' : undefined}
            title="Toggle member list"
            onClick={onToggleMembers}
          >
            <People size={20} />
          </button>
        </div>
      </div>
      {threadsOpen ? (
        <ThreadsPopover
          state={state}
          channel={channel}
          onClose={() => setThreadsOpen(false)}
          onOpenThread={(root) => {
            setThreadsOpen(false)
            onOpenThread(root)
          }}
          onCreate={() => {
            setThreadsOpen(false)
            onNewThread()
          }}
        />
      ) : null}
      <MessageList state={state} channel={channel} onReply={setReplyTarget} onOpenThread={onOpenThread} />
      <div style={{ position: 'relative' }}>
        <TypingIndicator state={state} channelId={channel.id} />
        <MessageInput
          state={state}
          channel={channel}
          replyTarget={replyTarget}
          onCancelReply={() => setReplyTarget(null)}
          onCreateThread={onNewThread}
        />
      </div>
    </div>
  )
}
