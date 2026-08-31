// Port of the chat reference PinnedMessages: header popover with search; each row jumps to the message.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Magnifier } from 'reicon-react'
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
    <div ref={panelRef} className="fc-threads-popover">
      <div className="fc-threads-header">
        <div className="fc-pins-title">
          <PinIcon size={16} />
          Pins
        </div>
        <div className="fc-threads-search">
          <Magnifier size={14} />
          <input value={query} placeholder="Search pinned messages" autoFocus onChange={(e) => setQuery(e.target.value)} />
        </div>
      </div>
      <div className="fc-threads-list fc-pins-scroll">
        {pins.length === 0 ? (
          <div className="fc-threads-empty">
            <PinIcon size={40} style={{ opacity: 0.3 }} />
            <h3>No pinned messages</h3>
            <p>Pinned messages in this channel will appear here.</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className="fc-threads-empty">
            <Magnifier size={40} style={{ opacity: 0.3 }} />
            <h3>No pinned messages found</h3>
            <p>Try another search.</p>
          </div>
        ) : (
          <div className="fc-pins-list">
            <div className="fc-threads-group-label" style={{ padding: 0 }}>
              {filtered.length} Pinned {filtered.length === 1 ? 'Message' : 'Messages'}
            </div>
            {filtered.map((msg) => {
              const author = authorUser(state, msg)
              return (
                <button
                  key={msg.id}
                  type="button"
                  className="fc-pins-row"
                  onClick={() => {
                    onClose()
                    requestAnimationFrame(() => jumpToMessage(msg.id))
                  }}
                >
                  <span
                    className="fc-threads-avatar"
                    style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
                  >
                    {displayName(state, msg).charAt(0).toUpperCase()}
                  </span>
                  <span className="fc-threads-row-body">
                    <span className="fc-threads-row-title">{extractPreview(msg.content) || 'Pinned message'}</span>
                    <span className="fc-pins-row-meta">
                      <span className="fc-pins-row-author">{displayName(state, msg)}</span> · {formatRelative(msg.createdAt)}
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
