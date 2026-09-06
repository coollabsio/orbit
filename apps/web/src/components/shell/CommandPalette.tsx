import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  DirectInbox,
  Home2,
  People,
  SearchNormal,
  Setting2,
  ShieldTick,
  TaskSquare,
  Trash,
} from 'reicon-react'
import type { IconComponent } from 'reicon-react'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { useProjects } from '../../features/tasks/api/projects'
import { taskFromRecord } from '../../features/tasks/api/models'
import { useTasks } from '../../features/tasks/api/tasks'

interface CommandEntry {
  id: string
  icon: IconComponent
  title: string
  meta: string
  to: string
  keywords: string
}

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id)
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const taskQuery = useTasks(workspace.id, { search: query || undefined, limit: 25 })
  const [activeIndex, setActiveIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const entries = useMemo<CommandEntry[]>(() => {
    const nav: CommandEntry[] = [
      { id: 'nav_home', icon: Home2, title: 'Go to Home', meta: 'Navigation', to: '/', keywords: 'home' },
      { id: 'nav_tasks', icon: TaskSquare, title: 'Go to Tasks', meta: 'Navigation', to: '/tasks', keywords: 'tasks' },
      { id: 'nav_inbox', icon: DirectInbox, title: 'Go to Inbox', meta: 'Navigation', to: '/inbox', keywords: 'inbox notifications' },
      { id: 'nav_profile', icon: People, title: 'Go to Profile', meta: 'Navigation', to: '/profile', keywords: 'profile account password name' },
      { id: 'nav_settings', icon: Setting2, title: 'Go to Settings', meta: 'Navigation', to: '/settings', keywords: 'settings preferences' },
      { id: 'nav_members', icon: People, title: 'Go to Members', meta: 'Navigation', to: '/settings/members', keywords: 'members invitations people' },
      { id: 'nav_sessions', icon: ShieldTick, title: 'Go to Sessions', meta: 'Navigation', to: '/settings/sessions', keywords: 'sessions devices' },
      { id: 'nav_trash', icon: Trash, title: 'Go to Task trash', meta: 'Navigation', to: '/tasks-trash', keywords: 'trash deleted tasks' },
    ]
    const tasks: CommandEntry[] = (taskQuery.data?.pages.flatMap((page) => page.items) ?? []).map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id))).map((t) => ({
      id: t.id,
      icon: TaskSquare,
      title: t.title,
      meta: t.identifier,
      to: `/tasks/${t.id}`,
      keywords: `${t.identifier} ${t.labels.join(' ')}`,
    }))
    return [...nav, ...tasks]
  }, [projects.data, taskQuery.data])

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
            placeholder="Search tasks and navigation…"
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
