// Port of the chat reference ChatArea: h-12 header (# + name + topic; right: Files, Threads, Pins, Members,
// search box) over MessageList + TypingIndicator + MessageInput, with file drag & drop.
import { useRef, useState, type DragEvent, type ReactNode } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { ArrowLeft, Folder, Hashtag, Magnifier, Paperclip2, People, Pin, Xmark } from 'reicon-react'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { FilesView } from './FilesView'
import { MessageInput, type MessageInputHandle } from './MessageInput'
import { MessageList } from './MessageList'
import { PinnedMessages } from './PinnedMessages'
import { SearchPanel } from './SearchPanel'
import { ThreadsPopover } from './ThreadsPopover'
import { TypingIndicator } from './TypingIndicator'

function eventHasFiles(event: DragEvent<HTMLElement>) {
  return Array.from(event.dataTransfer.types).includes('Files')
}

export function ChatArea({
  state,
  channel,
  membersOpen,
  onToggleMembers,
  onOpenThread,
  onNewThread,
  rightPanel,
}: {
  state: AppState
  channel: Channel
  membersOpen: boolean
  onToggleMembers: () => void
  onOpenThread: (root: ChatMessage) => void
  onNewThread: () => void
  /** the chat reference rightPanel: the member list renders under the full-width header, beside the messages */
  rightPanel?: ReactNode
}) {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const [replyTarget, setReplyTarget] = useState<ChatMessage | null>(null)
  const [threadsOpen, setThreadsOpen] = useState(false)
  const [pinsOpen, setPinsOpen] = useState(false)
  const [filesOpen, setFilesOpen] = useState(false)
  // ?q= opens the search side panel with a query (shareable)
  const [searchQuery, setSearchQuery] = useState(() => searchParams.get('q') ?? '')
  const [searchOpen, setSearchOpen] = useState(() => Boolean(searchParams.get('q')))
  const [draggingFiles, setDraggingFiles] = useState(false)
  const dragDepthRef = useRef(0)
  const inputRef = useRef<MessageInputHandle>(null)

  function handleDragEnter(event: DragEvent<HTMLDivElement>) {
    if (!eventHasFiles(event)) return
    event.preventDefault()
    dragDepthRef.current += 1
    setDraggingFiles(true)
  }
  function handleDragOver(event: DragEvent<HTMLDivElement>) {
    if (!eventHasFiles(event)) return
    event.preventDefault()
    event.dataTransfer.dropEffect = 'copy'
  }
  function handleDragLeave(event: DragEvent<HTMLDivElement>) {
    if (!eventHasFiles(event)) return
    event.preventDefault()
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1)
    if (dragDepthRef.current === 0) setDraggingFiles(false)
  }
  function handleDrop(event: DragEvent<HTMLDivElement>) {
    if (!eventHasFiles(event)) return
    event.preventDefault()
    const files = Array.from(event.dataTransfer.files)
    dragDepthRef.current = 0
    setDraggingFiles(false)
    if (files.length > 0) inputRef.current?.addFiles(files)
  }

  const headerButton = (active: boolean) => ({ className: 'fc-header-button', 'data-active': active ? 'true' : undefined })

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
            {...headerButton(filesOpen)}
            title="Files"
            onClick={() => {
              setFilesOpen(true)
              setThreadsOpen(false)
              setPinsOpen(false)
              setSearchOpen(false)
            }}
          >
            <Folder size={20} weight="Filled" />
          </button>
          <button
            type="button"
            {...headerButton(threadsOpen)}
            title="Threads"
            onClick={() => {
              setThreadsOpen((o) => !o)
              setPinsOpen(false)
            }}
          >
            <ThreadIcon size={20} />
          </button>
          <button
            type="button"
            {...headerButton(pinsOpen)}
            title="Pinned Messages"
            onClick={() => {
              setPinsOpen((o) => !o)
              setThreadsOpen(false)
            }}
          >
            <Pin size={20} weight="Filled" />
          </button>
          <button type="button" {...headerButton(membersOpen)} title="Toggle Member List" onClick={onToggleMembers}>
            <People size={20} weight="Filled" />
          </button>
          <div className="fc-header-search" data-open={searchOpen ? 'true' : undefined}>
            <Magnifier size={16} />
            <input
              type="text"
              value={searchQuery}
              placeholder="Search"
              onFocus={() => {
                setFilesOpen(false)
                setSearchOpen(true)
              }}
              onChange={(e) => {
                setFilesOpen(false)
                setSearchQuery(e.target.value)
                setSearchOpen(true)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setSearchOpen(false)
                  e.currentTarget.blur()
                }
              }}
            />
            {searchQuery || searchOpen ? (
              <button
                type="button"
                className="fc-header-search-clear"
                title="Clear search"
                onClick={() => {
                  setSearchQuery('')
                  setSearchOpen(false)
                }}
              >
                <Xmark size={14} />
              </button>
            ) : null}
          </div>
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
      {pinsOpen ? <PinnedMessages state={state} channel={channel} onClose={() => setPinsOpen(false)} /> : null}

      <div className="fc-chat-body">
      {filesOpen ? (
        <FilesView state={state} channel={channel} onBack={() => setFilesOpen(false)} />
      ) : (
        <>
        <div
          className="fc-drop-zone"
          onDragEnter={handleDragEnter}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
        >
          {draggingFiles ? (
            <div className="fc-drop-overlay">
              <div className="fc-drop-overlay-card">
                <Paperclip2 size={20} />
                Drop files to upload
              </div>
            </div>
          ) : null}
          <MessageList state={state} channel={channel} onReply={setReplyTarget} onOpenThread={onOpenThread} />
          <div style={{ position: 'relative' }}>
            <TypingIndicator state={state} channelId={channel.id} />
            <MessageInput
              ref={inputRef}
              state={state}
              channel={channel}
              replyTarget={replyTarget}
              onCancelReply={() => setReplyTarget(null)}
              onCreateThread={onNewThread}
            />
          </div>
        </div>
        {searchOpen ? <SearchPanel state={state} query={searchQuery} onClose={() => setSearchOpen(false)} /> : rightPanel}
        </>
      )}
      </div>
    </div>
  )
}
