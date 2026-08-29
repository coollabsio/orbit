// Port of the chat reference ThreadsPopover: header dropdown listing the channel's threads,
// grouped into Active (activity within 24h) and Inactive, with search and Create.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Magnifier } from 'reicon-react'
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
      <section className="fc-threads-group">
        <div className="fc-threads-group-label">{label}</div>
        {entries.map(({ root, lastReply }) => {
          const preview = lastReply ?? root
          const author = authorUser(state, preview)
          return (
            <button key={root.id} type="button" className="fc-threads-row" onClick={() => onOpenThread(root)}>
              <span
                className="fc-threads-avatar"
                style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
              >
                {displayName(state, preview).charAt(0).toUpperCase()}
              </span>
              <span className="fc-threads-row-body">
                <span className="fc-threads-row-title">{threadTitleOf(root)}</span>
                <span className="fc-threads-row-preview">
                  <strong>{displayName(state, preview)}:</strong> {extractPreview(preview.content) || 'No message content'}
                </span>
                <span className="fc-threads-row-time">Last updated {relativeTime(preview.createdAt)} ago</span>
              </span>
            </button>
          )
        })}
      </section>
    )
  }

  return (
    <div ref={ref} className="fc-threads-popover">
      <div className="fc-threads-header">
        <div className="fc-threads-search">
          <Magnifier size={14} />
          <input value={query} placeholder="Search for Thread Name" autoFocus onChange={(e) => setQuery(e.target.value)} />
        </div>
        <button type="button" className="fc-threads-create" onClick={onCreate}>
          Create
        </button>
      </div>
      <div className="fc-threads-list">
        {filtered.length === 0 ? (
          <div className="fc-threads-empty">
            <ThreadIcon size={28} />
            <h3>No threads</h3>
            <p>Threads with replies in this channel will appear here.</p>
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
