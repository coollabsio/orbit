// Port of the chat reference SearchOverlay (sidePanel + headerless): resizable right panel that lists
// workspace-wide message matches grouped by channel / thread; a card click jumps to the message.
import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { Hash, Search } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import type { AppState, ChatMessage } from '@/mock/types'
import { authorUser, displayName, jumpToMessage } from '@/features/chat/chatLib'
import { threadTitleOf } from '@/lib/messagePreview'

const WIDTH_KEY = 'orbit:search_panel_width'
const MIN_WIDTH = 240
const MAX_WIDTH = 480
const DEFAULT_WIDTH = 260

function clampWidth(width: number) {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width))
}

function storedWidth() {
  const value = Number(window.localStorage.getItem(WIDTH_KEY))
  return Number.isFinite(value) && value > 0 ? clampWidth(value) : DEFAULT_WIDTH
}

interface ResultGroup {
  key: string
  kind: 'channel' | 'thread'
  label: string
  results: ChatMessage[]
}

function groupResults(state: AppState, results: ChatMessage[]): ResultGroup[] {
  const groups = new Map<string, ResultGroup>()
  for (const result of results) {
    const threadId = result.threadRootId || (result.startsThread ? result.id : null)
    const kind = threadId ? 'thread' : 'channel'
    const key = `${kind}:${threadId || result.channelId}`
    const channel = state.channels.find((c) => c.id === result.channelId)
    const channelLabel = channel ? `# ${channel.name}` : 'Unknown channel'
    const label = kind === 'thread' ? `Thread in ${channelLabel}` : channelLabel
    const existing = groups.get(key)
    if (existing) existing.results.push(result)
    else groups.set(key, { key, kind, label, results: [result] })
  }
  return Array.from(groups.values())
}

const URL_REGEX = /https?:\/\/[^\s<>"']+/gi

function renderPreviewText(text: string): React.ReactNode {
  const nodes: React.ReactNode[] = []
  let lastIndex = 0
  for (const match of text.matchAll(URL_REGEX)) {
    const rawUrl = match[0]
    const index = match.index ?? 0
    const trailing = rawUrl.match(/[.,)\]};:!?]+$/)?.[0] ?? ''
    const url = trailing ? rawUrl.slice(0, -trailing.length) : rawUrl
    if (index > lastIndex) nodes.push(text.slice(lastIndex, index))
    nodes.push(
      <span key={`${url}-${index}`} className="font-medium text-primary">
        {url}
      </span>,
    )
    if (trailing) nodes.push(trailing)
    lastIndex = index + rawUrl.length
  }
  if (lastIndex < text.length) nodes.push(text.slice(lastIndex))
  return nodes.length > 0 ? nodes : text
}

function formatResultTime(iso: string): string {
  const date = new Date(iso)
  const now = new Date()
  const sameDay =
    date.getFullYear() === now.getFullYear() && date.getMonth() === now.getMonth() && date.getDate() === now.getDate()
  if (sameDay) return date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', hour12: true })
  return date.toLocaleString([], { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' })
}

export function SearchPanel({ state, query, onClose }: { state: AppState; query: string; onClose: () => void }) {
  const navigate = useNavigate()
  const [width, setWidth] = useState(storedWidth)
  // the chat reference debounces the API call by 300ms; the mock search is instant, so debounce the query itself
  const [debounced, setDebounced] = useState(query)

  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(query), 300)
    return () => window.clearTimeout(timer)
  }, [query])

  useEffect(() => {
    window.localStorage.setItem(WIDTH_KEY, String(width))
  }, [width])

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  const normalized = debounced.trim().toLowerCase()
  const searched = normalized.length > 0
  const results = useMemo(
    () =>
      searched
        ? state.chatMessages
            .filter((m) => m.content.toLowerCase().includes(normalized) || displayName(state, m).toLowerCase().includes(normalized))
            .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
        : [],
    [state, normalized, searched],
  )
  const groups = useMemo(() => groupResults(state, results), [state, results])

  function handleResizeStart(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault()
    function onMove(moveEvent: PointerEvent) {
      setWidth(clampWidth(window.innerWidth - moveEvent.clientX))
    }
    function onUp() {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  function openResult(result: ChatMessage) {
    const threadRootId = result.threadRootId || (result.startsThread ? result.id : null)
    const params = new URLSearchParams({ message_id: result.id })
    if (threadRootId) params.set('thread', threadRootId)
    onClose()
    navigate(`/chat/${result.channelId}?${params}`)
    requestAnimationFrame(() => jumpToMessage(result.id))
  }

  return (
    <div className="relative flex h-full shrink-0 flex-col border-l border-border bg-background max-[899px]:fixed max-[899px]:inset-x-0 max-[899px]:top-12 max-[899px]:bottom-0 max-[899px]:z-[45] max-[899px]:w-full! max-[899px]:border-l-0" style={{ width }}>
      <div className="absolute inset-y-0 left-0 z-20 w-1 cursor-col-resize transition-colors hover:bg-primary/40" onPointerDown={handleResizeStart} title="Resize search panel" />
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {searched && results.length > 0 ? (
          <div className="mb-2 px-1 text-xs leading-5 font-semibold tracking-wider text-muted-foreground uppercase">
            {results.length} result{results.length === 1 ? '' : 's'}
          </div>
        ) : null}

        {searched && results.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-12 text-center text-muted-foreground">
            <Search className="size-12 opacity-30" />
            <p className="text-sm text-muted-foreground">No results found for "{debounced.trim()}"</p>
          </div>
        ) : null}

        {!searched ? (
          <div className="flex flex-col items-center gap-3 py-12 text-center text-muted-foreground">
            <Search className="size-12 opacity-30" />
            <p className="text-sm text-muted-foreground">Type to search messages in this workspace</p>
          </div>
        ) : null}

        {groups.map((group) => (
          <section key={group.key} className="mb-4">
            <div className="mb-2 flex min-w-0 items-center gap-1.5 px-0.5 text-xs leading-5 font-bold text-foreground [&>svg]:shrink-0 [&>svg]:text-muted-foreground">
              {group.kind === 'thread' ? <ThreadIcon size={16} /> : <Hash size={16} />}
              <span className="truncate">{group.label}</span>
            </div>
            <div className="flex flex-col gap-2">
              {group.results.map((result) => {
                const author = authorUser(state, result)
                const name = displayName(state, result)
                const preview = result.startsThread && !result.threadRootId ? threadTitleOf(result) : result.content
                return (
                  <Button key={result.id} type="button" variant="ghost" className="flex h-auto w-full items-start justify-start gap-2.5 rounded-lg border border-border/70 bg-muted/25 bg-clip-border p-2.5 text-left font-normal whitespace-normal transition-colors hover:bg-muted/60 active:not-aria-[haspopup]:translate-y-0 dark:hover:bg-muted/60" onClick={() => openResult(result)}>
                    <span
                      className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold text-foreground"
                      style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
                    >
                      {name.charAt(0).toUpperCase()}
                    </span>
                    <span className="flex min-w-0 flex-1 flex-col">
                      <span className="flex min-w-0 items-baseline gap-1.5">
                        <span className="min-w-0 truncate text-sm leading-5 font-semibold text-foreground">{name}</span>
                        <span className="shrink-0 text-xs text-muted-foreground">{formatResultTime(result.createdAt)}</span>
                      </span>
                      <span className="mt-0.5 line-clamp-3 text-xs leading-5 text-foreground [overflow-wrap:anywhere]">{renderPreviewText(preview)}</span>
                    </span>
                  </Button>
                )
              })}
            </div>
          </section>
        ))}
      </div>
    </div>
  )
}
