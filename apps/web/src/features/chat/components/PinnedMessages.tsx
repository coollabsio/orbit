// Port of the chat reference PinnedMessages: header popover with search; each row jumps to the message.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { PinIcon } from '../../../components/ui/icons/PinIcon'
import type { AppState, Channel } from '../../../mock/types'
import { authorUser, displayName, extractPreview, jumpToMessage } from '../chatLib'

function formatRelative(iso: string): string {
  const diff = Math.max(0, Date.now() - new Date(iso).getTime())
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return 'now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

export function PinnedMessages({ state, channel, onClose }: { state: AppState; channel: Channel; onClose: () => void }) {
  const [query, setQuery] = useState('')
  const panelRef = useRef<HTMLDivElement>(null)

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

  useEffect(() => {
    function onPointerDown(e: MouseEvent) {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) onClose()
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [onClose])

  return (
    <div ref={panelRef} className="absolute top-12 right-4 z-40 flex max-h-[78vh] w-[544px] max-w-[calc(100vw-32px)] flex-col rounded-xl border border-border bg-popover shadow-xl max-[899px]:top-11 max-[899px]:right-2 max-[899px]:max-h-[calc(100dvh-108px)] max-[899px]:w-[calc(100vw-16px)] max-[899px]:max-w-none max-[899px]:rounded-[10px]">
      <div className="flex items-center gap-2 border-b border-border p-3 max-[899px]:gap-1.5 max-[899px]:p-2">
        <div className="flex shrink-0 items-center gap-2 text-sm font-bold text-foreground max-[899px]:text-xs [&>svg]:text-muted-foreground">
          <PinIcon size={16} />
          Pins
        </div>
        <div className="flex h-8 flex-1 items-center gap-2 rounded-lg border border-input bg-muted px-2.5 text-muted-foreground/70 max-[899px]:h-[30px] max-[899px]:gap-1.5 max-[899px]:px-2">
          <Search className="size-3.5 shrink-0" />
          <input value={query} placeholder="Search pinned messages" autoFocus onChange={(e) => setQuery(e.target.value)} className="min-w-0 flex-1 border-none bg-transparent text-[13px] text-foreground outline-none max-[899px]:text-xs" />
        </div>
      </div>
      <div className="min-h-0 overflow-y-auto p-4 max-[899px]:p-1.5">
        {pins.length === 0 ? (
          <div className="flex flex-col items-center gap-1 p-10 text-center text-muted-foreground/70">
            <PinIcon size={40} style={{ opacity: 0.3 }} />
            <h3 className="mt-2 text-[15px] font-semibold text-foreground">No pinned messages</h3>
            <p className="text-[13px] text-muted-foreground">Pinned messages in this channel will appear here.</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-col items-center gap-1 p-10 text-center text-muted-foreground/70">
            <Search className="size-10 opacity-30" />
            <h3 className="mt-2 text-[15px] font-semibold text-foreground">No pinned messages found</h3>
            <p className="text-[13px] text-muted-foreground">Try another search.</p>
          </div>
        ) : (
          <div className="flex flex-col gap-3 max-[899px]:gap-[7px]">
            <div className="text-[11px] font-bold tracking-wider text-muted-foreground uppercase">
              {filtered.length} Pinned {filtered.length === 1 ? 'Message' : 'Messages'}
            </div>
            {filtered.map((msg) => {
              const author = authorUser(state, msg)
              return (
                <button
                  key={msg.id}
                  type="button"
                  className="flex w-full items-center gap-3 rounded-lg border border-border bg-muted/20 p-3 text-left transition-colors hover:bg-muted/50 max-[899px]:gap-2 max-[899px]:p-[7px]"
                  onClick={() => {
                    onClose()
                    requestAnimationFrame(() => jumpToMessage(msg.id))
                  }}
                >
                  <span
                    className="flex size-9 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-bold text-muted-foreground max-[899px]:size-[30px] max-[899px]:text-[11px]"
                    style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
                  >
                    {displayName(state, msg).charAt(0).toUpperCase()}
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-sm font-bold text-foreground max-[899px]:text-xs">{extractPreview(msg.content) || 'Pinned message'}</span>
                    <span className="truncate text-xs text-muted-foreground max-[899px]:text-[10px]">
                      <span className="font-semibold text-primary">{displayName(state, msg)}</span> · {formatRelative(msg.createdAt)}
                    </span>
                  </span>
                </button>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
