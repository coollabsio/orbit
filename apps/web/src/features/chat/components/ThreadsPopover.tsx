// Port of the chat reference ThreadsPopover: header dropdown listing the channel's threads,
// grouped into Active (activity within 24h) and Inactive, with search and Create.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import { relativeTime } from '../../../lib/format'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { authorUser, displayName, extractPreview, threadTitleOf } from '../chatLib'

const DAY_MS = 86_400_000

interface ThreadEntry {
  root: ChatMessage
  lastReply: ChatMessage | null
  lastActivity: number
}

export function ThreadsPopover({
  state,
  channel,
  onOpenThread,
  onCreate,
  onClose,
}: {
  state: AppState
  channel: Channel
  onOpenThread: (root: ChatMessage) => void
  onCreate: () => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [openedAt] = useState(() => Date.now())
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function onPointerDown(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
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
            <button key={root.id} type="button" className="flex w-full items-start gap-3 rounded-lg p-2 text-left transition-colors hover:bg-muted max-[899px]:gap-2 max-[899px]:p-[7px]" onClick={() => onOpenThread(root)}>
              <span
                className="flex size-9 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-bold text-muted-foreground max-[899px]:size-[30px] max-[899px]:text-[11px]"
                style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
              >
                {displayName(state, preview).charAt(0).toUpperCase()}
              </span>
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-sm font-bold text-foreground max-[899px]:text-xs">{threadTitleOf(root)}</span>
                <span className="truncate text-[13px] text-muted-foreground max-[899px]:text-[10px] [&>strong]:text-foreground">
                  <strong>{displayName(state, preview)}:</strong> {extractPreview(preview.content) || 'No message content'}
                </span>
                <span className="text-[11px] text-muted-foreground max-[899px]:text-[9px]">Last updated {relativeTime(preview.createdAt)} ago</span>
              </span>
            </button>
          )
        })}
      </section>
    )
  }

  return (
    <div ref={ref} className="absolute top-12 right-4 z-40 flex max-h-[78vh] w-[544px] max-w-[calc(100vw-32px)] flex-col rounded-xl border border-border bg-popover shadow-xl max-[899px]:top-11 max-[899px]:right-2 max-[899px]:max-h-[calc(100dvh-108px)] max-[899px]:w-[calc(100vw-16px)] max-[899px]:max-w-none max-[899px]:rounded-[10px]">
      <div className="flex items-center gap-2 border-b border-border p-3 max-[899px]:gap-1.5 max-[899px]:p-2">
        <div className="flex h-8 flex-1 items-center gap-2 rounded-lg border border-input bg-muted px-2.5 text-muted-foreground/70 max-[899px]:h-[30px] max-[899px]:gap-1.5 max-[899px]:px-2">
          <Search className="size-3.5 shrink-0" />
          <input value={query} placeholder="Search for Thread Name" autoFocus onChange={(e) => setQuery(e.target.value)} className="min-w-0 flex-1 border-none bg-transparent text-[13px] text-foreground outline-none max-[899px]:text-xs" />
        </div>
        <button type="button" className="h-8 rounded-lg bg-green-500 px-3 text-[13px] font-semibold text-white transition hover:brightness-[1.08] max-[899px]:h-[30px] max-[899px]:px-2.5 max-[899px]:text-[11px]" onClick={onCreate}>
          Create
        </button>
      </div>
      <div className="min-h-0 overflow-y-auto p-2 max-[899px]:p-1.5">
        {filtered.length === 0 ? (
          <div className="flex flex-col items-center gap-1 p-10 text-center text-muted-foreground/70 max-[899px]:px-3 max-[899px]:py-7 [&>svg]:max-[899px]:size-8">
            <ThreadIcon size={28} />
            <h3 className="mt-2 text-[15px] font-semibold text-foreground max-[899px]:text-[13px]">No threads</h3>
            <p className="text-[13px] text-muted-foreground max-[899px]:text-[11px]">Threads with replies in this channel will appear here.</p>
          </div>
        ) : (
          <>
            {renderGroup('Active', active)}
            {renderGroup('Inactive', inactive)}
          </>
        )}
      </div>
    </div>
  )
}
