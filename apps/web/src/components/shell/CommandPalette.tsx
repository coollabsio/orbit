import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  DirectInbox,
  Hashtag,
  Home2,
  Note2,
  SearchNormal,
  Setting2,
  Sms,
  TaskSquare,
} from 'reicon-react'
import type { IconComponent } from 'reicon-react'
import { useAppState } from '../../mock/store'

interface CommandEntry {
  id: string
  icon: IconComponent
  title: string
  meta: string
  to: string
  keywords: string
}

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const state = useAppState()
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const entries = useMemo<CommandEntry[]>(() => {
    const nav: CommandEntry[] = [
      { id: 'nav_home', icon: Home2, title: 'Go to Home', meta: 'Navigation', to: '/', keywords: 'home' },
      { id: 'nav_tasks', icon: TaskSquare, title: 'Go to Tasks', meta: 'Navigation', to: '/tasks', keywords: 'tasks' },
      { id: 'nav_docs', icon: Note2, title: 'Go to Docs', meta: 'Navigation', to: '/docs', keywords: 'docs documents' },
      { id: 'nav_mail', icon: Sms, title: 'Go to Mail', meta: 'Navigation', to: '/mail', keywords: 'mail email' },
      { id: 'nav_chat', icon: Hashtag, title: 'Go to Chat', meta: 'Navigation', to: '/chat', keywords: 'chat channels' },
      { id: 'nav_inbox', icon: DirectInbox, title: 'Go to Inbox', meta: 'Navigation', to: '/inbox', keywords: 'inbox notifications' },
      { id: 'nav_settings', icon: Setting2, title: 'Go to Settings', meta: 'Navigation', to: '/settings', keywords: 'settings preferences' },
    ]
    const tasks: CommandEntry[] = state.tasks.map((t) => ({
      id: t.id,
      icon: TaskSquare,
      title: t.title,
      meta: t.identifier,
      to: `/tasks/${t.id}`,
      keywords: `${t.identifier} ${t.labels.join(' ')}`,
    }))
    const docs: CommandEntry[] = state.docs.map((d) => ({
      id: d.id,
      icon: Note2,
      title: d.title,
      meta: 'Doc',
      to: `/docs/${d.id}`,
      keywords: 'doc document',
    }))
    const mail: CommandEntry[] = state.mailThreads.map((m) => ({
      id: m.id,
      icon: Sms,
      title: m.subject,
      meta: 'Mail',
      to: `/mail/${m.id}`,
      keywords: `mail ${m.messages[0]?.from.name ?? ''}`,
    }))
    const channels: CommandEntry[] = state.channels.map((c) => ({
      id: c.id,
      icon: Hashtag,
      title: `#${c.name}`,
      meta: 'Channel',
      to: `/chat/${c.id}`,
      keywords: `channel chat ${c.name}`,
    }))
    return [...nav, ...tasks, ...docs, ...mail, ...channels]
  }, [state])

  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return entries.slice(0, 9)
    return entries
      .filter((e) => `${e.title} ${e.meta} ${e.keywords}`.toLowerCase().includes(q))
      .slice(0, 12)
  }, [entries, query])

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const open = (entry: CommandEntry | undefined) => {
    if (!entry) return
    onClose()
    navigate(entry.to)
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') onClose()
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActiveIndex((i) => Math.min(i + 1, results.length - 1))
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((i) => Math.max(i - 1, 0))
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      open(results[activeIndex])
    }
  }

  useEffect(() => {
    const el = listRef.current?.querySelector('[data-active="true"]')
    el?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex])

  return (
    <div
      className="command-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="command-panel" onKeyDown={onKeyDown}>
        <div className="command-input-row">
          <SearchNormal size={16} />
          <input
            ref={inputRef}
            className="command-input"
            placeholder="Search tasks, docs, mail, channels…"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value)
              setActiveIndex(0)
            }}
          />
          <span className="kbd">esc</span>
        </div>
        <div className="command-list" ref={listRef}>
          {results.length === 0 ? (
            <div className="command-empty">No results for “{query}”</div>
          ) : (
            results.map((entry, index) => (
              <button
                key={entry.id}
                className="command-item"
                data-active={index === activeIndex}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => open(entry)}
              >
                <entry.icon size={16} />
                <span className="truncate">{entry.title}</span>
                <span className="command-item-meta">{entry.meta}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
