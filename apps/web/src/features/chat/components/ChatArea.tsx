// Port of the chat reference ChatArea: h-12 header (# + name + topic; right: Files, Threads, Pins, Members,
// search box) over MessageList + TypingIndicator + MessageInput, with file drag & drop.
import { useRef, useState, type DragEvent, type ReactNode } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { ArrowLeft, Folder, Hash, Paperclip, Search, Users, X } from 'lucide-react'
import { PinIcon } from '../../../components/ui/icons/PinIcon'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import type { AppState, Channel, ChatMessage, User } from '../../../mock/types'
import { UserAvatar } from '../../../components/ui/UserAvatar'
import { FilesView } from './FilesView'
import { MessageInput, type MessageInputHandle } from './MessageInput'
import { MessageList } from './MessageList'
import { Emoji } from '../../../components/ui/Emoji'
import { PinnedMessages } from './PinnedMessages'
import { SearchPanel } from './SearchPanel'
import { ThreadsPopover } from './ThreadsPopover'
import { TypingIndicator } from './TypingIndicator'

const headerButtonClass =
  'inline-flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[active=true]:text-primary data-[active=true]:hover:bg-primary/10 [&>svg]:size-5 max-[899px]:size-[30px] max-[899px]:[&>svg]:size-[17px]'

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
  dmParticipant,
}: {
  state: AppState
  channel: Channel
  membersOpen: boolean
  onToggleMembers: () => void
  onOpenThread: (root: ChatMessage) => void
  onNewThread: () => void
  /** the chat reference rightPanel: the member list renders under the full-width header, beside the messages */
  rightPanel?: ReactNode
  dmParticipant?: User
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

  const headerButton = (active: boolean) => ({ className: headerButtonClass, 'data-active': active ? 'true' : undefined })

  return (
    <div className="relative flex min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=list]/chat:hidden">
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-border bg-background pr-3 pl-4 max-[899px]:relative max-[899px]:px-2">
        <div className="flex min-w-0 items-center gap-3 max-[899px]:gap-[7px] [&>svg]:size-5 [&>svg]:shrink-0 [&>svg]:text-muted-foreground">
          <button type="button" className="-ml-1 hidden size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground max-[899px]:flex" title="Back to conversations" onClick={() => navigate(dmParticipant ? '/dm' : '/chat')}>
            <ArrowLeft className="size-[18px]" />
          </button>
          {dmParticipant ? <UserAvatar user={dmParticipant} size={28} showOnline /> : channel.emoji ? <span className="inline-flex size-5 items-center justify-center text-base leading-none"><Emoji value={channel.emoji} size={20} /></span> : <Hash />}
          <div className="flex min-w-0 items-baseline gap-2">
            <h2 className="text-sm font-semibold whitespace-nowrap text-foreground max-[899px]:max-w-[110px] max-[899px]:truncate max-[899px]:text-[13px]">{channel.name}</h2>
            {channel.description ? (
              <div className="flex min-w-0 items-baseline gap-1.5 border-l border-border pl-3 max-[899px]:hidden">
                <span className="truncate text-xs text-muted-foreground">{channel.description}</span>
              </div>
            ) : null}
          </div>
        </div>
        <div className="flex items-center gap-2 max-[899px]:gap-0">
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
            <Folder />
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
            <PinIcon size={20} />
          </button>
          {!dmParticipant ? (
            <button type="button" {...headerButton(membersOpen)} title="Toggle Member List" onClick={onToggleMembers}>
              <Users />
            </button>
          ) : null}
          <div
            className="group/search relative mx-1 flex h-8 w-56 items-center gap-1.5 rounded-lg border border-border bg-muted/40 px-2 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[open=true]:border-primary/50 data-[open=true]:bg-muted data-[open=true]:text-foreground max-[899px]:mx-0 max-[899px]:w-[30px] max-[899px]:cursor-pointer max-[899px]:justify-center max-[899px]:border-transparent max-[899px]:bg-transparent max-[899px]:px-0 data-[open=true]:max-[899px]:absolute data-[open=true]:max-[899px]:right-2 data-[open=true]:max-[899px]:left-2 data-[open=true]:max-[899px]:z-[5] data-[open=true]:max-[899px]:w-auto data-[open=true]:max-[899px]:cursor-text data-[open=true]:max-[899px]:justify-start data-[open=true]:max-[899px]:gap-1.5 data-[open=true]:max-[899px]:border-primary/50 data-[open=true]:max-[899px]:bg-muted data-[open=true]:max-[899px]:px-2"
            data-open={searchOpen ? 'true' : undefined}
            onClick={(event) => event.currentTarget.querySelector('input')?.focus()}
          >
            <Search className="size-4 shrink-0" />
            <input
              type="text"
              value={searchQuery}
              placeholder="Search"
              className="min-w-0 flex-1 border-none bg-transparent text-xs font-medium text-foreground outline-none max-[899px]:absolute max-[899px]:inset-0 max-[899px]:w-full max-[899px]:cursor-pointer max-[899px]:opacity-0 group-data-[open=true]/search:max-[899px]:static group-data-[open=true]/search:max-[899px]:w-auto group-data-[open=true]/search:max-[899px]:cursor-text group-data-[open=true]/search:max-[899px]:opacity-100"
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
                className="grid size-4 shrink-0 place-items-center rounded text-muted-foreground hover:text-foreground max-[899px]:hidden group-data-[open=true]/search:max-[899px]:grid"
                title="Clear search"
                onClick={() => {
                  setSearchQuery('')
                  setSearchOpen(false)
                }}
              >
                <X className="size-3.5" />
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

      <div className="flex min-h-0 flex-1">
      {filesOpen ? (
        <FilesView state={state} channel={channel} onBack={() => setFilesOpen(false)} />
      ) : (
        <>
        <div
          className="relative flex min-h-0 min-w-0 flex-1 flex-col"
          onDragEnter={handleDragEnter}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
        >
          {draggingFiles ? (
            <div className="pointer-events-none absolute inset-3 z-30 grid place-items-center rounded-xl border-2 border-dashed border-primary/70 bg-background/80 backdrop-blur-sm">
              <div className="flex items-center gap-3 rounded-xl border border-border bg-popover px-4 py-3 text-sm font-bold text-foreground shadow-xl [&>svg]:text-primary">
                <Paperclip className="size-5" />
                Drop files to upload
              </div>
            </div>
          ) : null}
          <MessageList state={state} channel={channel} onReply={setReplyTarget} onOpenThread={onOpenThread} />
          <div className="relative">
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
