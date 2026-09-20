// Port of the chat reference ChatArea: h-12 header (# + name + topic; right: Files, Threads, Pins, Members,
// search box) over MessageList + TypingIndicator + MessageInput, with file drag & drop.
import { useRef, useState, type DragEvent, type ReactNode } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { ArrowLeft, Folder, Hash, Paperclip, Search, Users, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverTrigger } from '@/components/ui/popover'
import { PinIcon } from '@/components/common/icons/PinIcon'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import type { AppState, Channel, ChatMessage, User } from '@/mock/types'
import { UserAvatar } from '@/components/common/UserAvatar'
import { FilesView } from './FilesView'
import { MessageInput, type MessageInputHandle } from './MessageInput'
import { MessageList } from './MessageList'
import { Emoji } from '@/components/common/Emoji'
import { PinnedMessages } from './PinnedMessages'
import { SearchPanel } from './SearchPanel'
import { ThreadsPopover } from './ThreadsPopover'
import { TypingIndicator } from './TypingIndicator'

const headerButtonClass =
  "rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground dark:hover:bg-muted data-[active=true]:text-primary data-[active=true]:hover:bg-primary/10 dark:data-[active=true]:hover:bg-primary/10 [&_svg:not([class*='size-'])]:size-5 max-[899px]:size-[30px] max-[899px]:[&_svg:not([class*='size-'])]:size-[17px]"

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
  const headerRef = useRef<HTMLDivElement>(null)

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

  const headerButton = (active: boolean) => ({
    variant: 'ghost' as const,
    size: 'icon' as const,
    className: headerButtonClass,
    'data-active': active ? 'true' : undefined,
  })

  return (
    <div className="relative flex min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=list]/chat:hidden">
      <div ref={headerRef} className="flex h-12 shrink-0 items-center justify-between border-b border-border bg-background pr-3 pl-4 max-[899px]:relative max-[899px]:px-2">
        <div className="flex min-w-0 items-center gap-3 max-[899px]:gap-[7px] [&>svg]:size-5 [&>svg]:shrink-0 [&>svg]:text-muted-foreground">
          <Button type="button" variant="ghost" size="icon" className="-ml-1 hidden rounded-md text-muted-foreground hover:bg-muted hover:text-foreground dark:hover:bg-muted max-[899px]:flex" title="Back to conversations" onClick={() => navigate(dmParticipant ? '/dm' : '/chat')}>
            <ArrowLeft className="size-[18px]" />
          </Button>
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
          <Button
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
          </Button>
          <Popover
            open={threadsOpen}
            onOpenChange={(open) => {
              setThreadsOpen(open)
              if (open) setPinsOpen(false)
            }}
          >
            <PopoverTrigger render={<Button type="button" {...headerButton(threadsOpen)} title="Threads" />}>
              <ThreadIcon size={20} />
            </PopoverTrigger>
            <ThreadsPopover
              anchor={headerRef}
              state={state}
              channel={channel}
              onOpenThread={(root) => {
                setThreadsOpen(false)
                onOpenThread(root)
              }}
              onCreate={() => {
                setThreadsOpen(false)
                onNewThread()
              }}
            />
          </Popover>
          <Popover
            open={pinsOpen}
            onOpenChange={(open) => {
              setPinsOpen(open)
              if (open) setThreadsOpen(false)
            }}
          >
            <PopoverTrigger render={<Button type="button" {...headerButton(pinsOpen)} title="Pinned Messages" />}>
              <PinIcon size={20} />
            </PopoverTrigger>
            <PinnedMessages anchor={headerRef} state={state} channel={channel} onClose={() => setPinsOpen(false)} />
          </Popover>
          {!dmParticipant ? (
            <Button type="button" {...headerButton(membersOpen)} title="Toggle Member List" onClick={onToggleMembers}>
              <Users />
            </Button>
          ) : null}
          <div
            className="group/search relative mx-1 flex h-8 w-56 items-center gap-1.5 rounded-lg border border-border bg-muted/40 px-2 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[open=true]:border-primary/50 data-[open=true]:bg-muted data-[open=true]:text-foreground max-[899px]:mx-0 max-[899px]:w-[30px] max-[899px]:cursor-pointer max-[899px]:justify-center max-[899px]:border-transparent max-[899px]:bg-transparent max-[899px]:px-0 data-[open=true]:max-[899px]:absolute data-[open=true]:max-[899px]:right-2 data-[open=true]:max-[899px]:left-2 data-[open=true]:max-[899px]:z-[5] data-[open=true]:max-[899px]:w-auto data-[open=true]:max-[899px]:cursor-text data-[open=true]:max-[899px]:justify-start data-[open=true]:max-[899px]:gap-1.5 data-[open=true]:max-[899px]:border-primary/50 data-[open=true]:max-[899px]:bg-muted data-[open=true]:max-[899px]:px-2"
            data-open={searchOpen ? 'true' : undefined}
            onClick={(event) => event.currentTarget.querySelector('input')?.focus()}
          >
            <Search className="size-4 shrink-0" />
            <Input
              type="text"
              value={searchQuery}
              placeholder="Search"
              className="h-auto w-auto min-w-0 flex-1 rounded-none border-0 bg-transparent p-0 text-xs font-medium text-foreground shadow-none outline-none focus-visible:ring-0 md:text-xs dark:bg-transparent max-[899px]:absolute max-[899px]:inset-0 max-[899px]:w-full max-[899px]:cursor-pointer max-[899px]:opacity-0 group-data-[open=true]/search:max-[899px]:static group-data-[open=true]/search:max-[899px]:w-auto group-data-[open=true]/search:max-[899px]:cursor-text group-data-[open=true]/search:max-[899px]:opacity-100"
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
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                className="grid size-4 shrink-0 place-items-center rounded text-muted-foreground hover:bg-transparent hover:text-foreground dark:hover:bg-transparent max-[899px]:hidden group-data-[open=true]/search:max-[899px]:grid"
                title="Clear search"
                onClick={() => {
                  setSearchQuery('')
                  setSearchOpen(false)
                }}
              >
                <X className="size-3.5" />
              </Button>
            ) : null}
          </div>
        </div>
      </div>

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
