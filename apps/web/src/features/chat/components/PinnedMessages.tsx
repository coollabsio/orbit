// Port of the chat reference PinnedMessages: header popover with search; each row jumps to the message.
import { useMemo, useRef, useState, type RefObject } from 'react'
import { SearchNormal as Search } from 'reicon-react'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { PinIcon } from '@/components/common/icons/PinIcon'
import type { AppState, Channel } from '@/mock/types'
import { authorUser, displayName, jumpToMessage } from '@/features/chat/chatLib'
import { extractPreview } from '@/lib/messagePreview'
import { HeaderPopoverContent } from './HeaderPopoverContent'
import { InitialAvatar } from './InitialAvatar'
import { RowButton } from './RowButton'

function formatRelative(iso: string): string {
  const diff = Math.max(0, Date.now() - new Date(iso).getTime())
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return 'now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

interface PinnedMessagesProps {
  state: AppState
  channel: Channel
  onClose: () => void
}

/** Popover content for the header Pins button; render inside a `Popover` whose trigger is that button.
    The panel body mounts only while open, so the search resets on each open. */
export function PinnedMessages({ anchor, ...props }: PinnedMessagesProps & { anchor: RefObject<HTMLElement | null> }) {
  const searchRef = useRef<HTMLInputElement>(null)
  return (
    <HeaderPopoverContent anchor={anchor} initialFocus={searchRef}>
      <PinnedPanel {...props} searchRef={searchRef} />
    </HeaderPopoverContent>
  )
}

function PinnedPanel({ state, channel, onClose, searchRef }: PinnedMessagesProps & { searchRef: RefObject<HTMLInputElement | null> }) {
  const [query, setQuery] = useState('')

  const pins = useMemo(
    () =>
      state.chatMessages
        .filter((m) => m.channelId === channel.id && m.pinned)
        .sort((a, b) => (b.pinnedAt ?? b.createdAt).localeCompare(a.pinnedAt ?? a.createdAt)),
    [state.chatMessages, channel.id],
  )
  const normalized = query.trim().toLowerCase()
  const filtered = normalized
    ? pins.filter(
        (m) =>
          extractPreview(m.content).toLowerCase().includes(normalized) ||
          displayName(state, m).toLowerCase().includes(normalized),
      )
    : pins


  return (
    <>
      <div className="flex items-center gap-2 border-b border-border p-3 max-[899px]:gap-1.5 max-[899px]:p-2">
        <div className="flex shrink-0 items-center gap-2 text-sm font-bold text-foreground max-[899px]:text-xs [&>svg]:text-muted-foreground">
          <PinIcon size={16} />
          Pins
        </div>
        <InputGroup className="flex-1">
          <InputGroupAddon>
            <Search />
          </InputGroupAddon>
          <InputGroupInput ref={searchRef} value={query} placeholder="Search pinned messages" aria-label="Search pinned messages" onChange={(e) => setQuery(e.target.value)} />
        </InputGroup>
      </div>
      <div className="min-h-0 overflow-y-auto overscroll-contain p-4 max-[899px]:p-1.5">
        {pins.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <PinIcon />
              </EmptyMedia>
              <EmptyTitle>No pinned messages</EmptyTitle>
              <EmptyDescription>Pinned messages in this channel will appear here.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : filtered.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Search />
              </EmptyMedia>
              <EmptyTitle>No pinned messages found</EmptyTitle>
              <EmptyDescription>Try another search.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="flex flex-col gap-3 max-[899px]:gap-[7px]">
            <div className="text-[11px] font-bold tracking-wider text-muted-foreground uppercase">
              {filtered.length} Pinned {filtered.length === 1 ? 'Message' : 'Messages'}
            </div>
            {filtered.map((msg) => {
              const author = authorUser(state, msg)
              return (
                <RowButton
                  key={msg.id}
                  className="gap-3 rounded-lg border border-border bg-muted/20 p-3 hover:bg-muted/50 max-[899px]:gap-2 max-[899px]:p-[7px]"
                  onClick={() => {
                    onClose()
                    requestAnimationFrame(() => jumpToMessage(msg.id))
                  }}
                >
                  <InitialAvatar name={displayName(state, msg)} color={author?.color} className="size-9 text-sm font-bold max-[899px]:size-[30px] max-[899px]:text-[11px]" />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-sm font-bold text-foreground max-[899px]:text-xs">{extractPreview(msg.content) || 'Pinned message'}</span>
                    <span className="truncate text-xs text-muted-foreground max-[899px]:text-[10px]">
                      <span className="font-semibold text-primary">{displayName(state, msg)}</span> · {formatRelative(msg.createdAt)}
                    </span>
                  </span>
                </RowButton>
              )
            })}
          </div>
        )}
      </div>
    </>
  )
}
