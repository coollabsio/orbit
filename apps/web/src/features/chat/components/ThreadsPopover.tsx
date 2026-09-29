// Port of the chat reference ThreadsPopover: header dropdown listing the channel's threads,
// grouped into Active (activity within 24h) and Inactive, with search and Create.
import { useMemo, useRef, useState, type RefObject } from 'react'
import { SearchNormal as Search } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import { relativeTime } from '@/lib/format'
import type { AppState, Channel, ChatMessage } from '@/mock/types'
import { authorUser, displayName } from '@/features/chat/chatLib'
import { extractPreview, threadTitleOf } from '@/lib/messagePreview'
import { HeaderPopoverContent } from './HeaderPopoverContent'
import { InitialAvatar } from './InitialAvatar'
import { RowButton } from './RowButton'

const DAY_MS = 86_400_000

interface ThreadEntry {
  root: ChatMessage
  lastReply: ChatMessage | null
  lastActivity: number
}

interface ThreadsPopoverProps {
  state: AppState
  channel: Channel
  onOpenThread: (root: ChatMessage) => void
  onCreate: () => void
}

/** Popover content for the header Threads button; render inside a `Popover` whose trigger is that button.
    The panel body mounts only while open, so search and the Active/Inactive cutoff reset on each open. */
export function ThreadsPopover({ anchor, ...props }: ThreadsPopoverProps & { anchor: RefObject<HTMLElement | null> }) {
  const searchRef = useRef<HTMLInputElement>(null)
  return (
    <HeaderPopoverContent anchor={anchor} initialFocus={searchRef}>
      <ThreadsPanel {...props} searchRef={searchRef} />
    </HeaderPopoverContent>
  )
}

function ThreadsPanel({ state, channel, onOpenThread, onCreate, searchRef }: ThreadsPopoverProps & { searchRef: RefObject<HTMLInputElement | null> }) {
  const [query, setQuery] = useState('')
  const [openedAt] = useState(() => Date.now())


  const threads = useMemo<ThreadEntry[]>(() => {
    const inChannel = state.chatMessages.filter((m) => m.channelId === channel.id)
    return inChannel
      .filter((m) => !m.threadRootId && (m.startsThread || inChannel.some((r) => r.threadRootId === m.id)))
      .map((root) => {
        const replies = inChannel.filter((r) => r.threadRootId === root.id).sort((a, b) => a.createdAt.localeCompare(b.createdAt))
        const lastReply = replies[replies.length - 1] ?? null
        return { root, lastReply, lastActivity: new Date((lastReply ?? root).createdAt).getTime() }
      })
      .sort((a, b) => b.lastActivity - a.lastActivity)
  }, [state.chatMessages, channel.id])

  const normalized = query.trim().toLowerCase()
  const filtered = normalized ? threads.filter((t) => threadTitleOf(t.root).toLowerCase().includes(normalized)) : threads
  const active = filtered.filter((t) => openedAt - t.lastActivity < DAY_MS)
  const inactive = filtered.filter((t) => openedAt - t.lastActivity >= DAY_MS)

  function renderGroup(label: string, entries: ThreadEntry[]) {
    if (entries.length === 0) return null
    return (
      <section className="mb-4">
        <div className="px-2 pt-2 pb-1 text-[11px] font-bold tracking-wider text-muted-foreground uppercase max-[899px]:px-1.5 max-[899px]:pt-1.5 max-[899px]:pb-[3px] max-[899px]:text-[9px]">{label}</div>
        {entries.map(({ root, lastReply }) => {
          const preview = lastReply ?? root
          const author = authorUser(state, preview)
          return (
            <RowButton key={root.id} className="items-start gap-3 rounded-lg p-2 hover:bg-muted max-[899px]:gap-2 max-[899px]:p-[7px]" onClick={() => onOpenThread(root)}>
              <InitialAvatar name={displayName(state, preview)} color={author?.color} className="size-9 text-sm font-bold max-[899px]:size-[30px] max-[899px]:text-[11px]" />
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-sm font-bold text-foreground max-[899px]:text-xs">{threadTitleOf(root)}</span>
                <span className="truncate text-[13px] text-muted-foreground max-[899px]:text-[10px] [&>strong]:text-foreground">
                  <strong>{displayName(state, preview)}:</strong> {extractPreview(preview.content) || 'No message content'}
                </span>
                <span className="text-[11px] text-muted-foreground max-[899px]:text-[9px]">Last updated {relativeTime(preview.createdAt)} ago</span>
              </span>
            </RowButton>
          )
        })}
      </section>
    )
  }

  return (
    <>
      <div className="flex items-center gap-2 border-b border-border p-3 max-[899px]:gap-1.5 max-[899px]:p-2">
        <InputGroup className="flex-1">
          <InputGroupAddon>
            <Search />
          </InputGroupAddon>
          <InputGroupInput ref={searchRef} value={query} placeholder="Search for Thread Name" aria-label="Search threads" onChange={(e) => setQuery(e.target.value)} />
        </InputGroup>
        <Button type="button" onClick={onCreate}>
          Create
        </Button>
      </div>
      <div className="min-h-0 overflow-y-auto p-2 max-[899px]:p-1.5">
        {filtered.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <ThreadIcon />
              </EmptyMedia>
              <EmptyTitle>No threads</EmptyTitle>
              <EmptyDescription>Threads with replies in this channel will appear here.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <>
            {renderGroup('Active', active)}
            {renderGroup('Inactive', inactive)}
          </>
        )}
      </div>
    </>
  )
}
