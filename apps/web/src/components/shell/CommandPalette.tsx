import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  Inbox,
  Home,
  Users,
  Search,
  Settings,
  ShieldCheck,
  SquareCheck,
  Trash2,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { useProjects } from '../../features/tasks/api/projects'
import { taskFromRecord } from '../../features/tasks/api/models'
import { useTasks } from '../../features/tasks/api/tasks'

interface CommandEntry {
  id: string
  icon: LucideIcon
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
      { id: 'nav_home', icon: Home, title: 'Go to Home', meta: 'Navigation', to: '/', keywords: 'home' },
      { id: 'nav_tasks', icon: SquareCheck, title: 'Go to Tasks', meta: 'Navigation', to: '/tasks', keywords: 'tasks' },
      { id: 'nav_inbox', icon: Inbox, title: 'Go to Inbox', meta: 'Navigation', to: '/inbox', keywords: 'inbox notifications' },
      { id: 'nav_profile', icon: Users, title: 'Go to Profile', meta: 'Navigation', to: '/profile', keywords: 'profile account password name' },
      { id: 'nav_settings', icon: Settings, title: 'Go to Settings', meta: 'Navigation', to: '/settings', keywords: 'settings preferences' },
      { id: 'nav_members', icon: Users, title: 'Go to Members', meta: 'Navigation', to: '/settings/members', keywords: 'members invitations people' },
      { id: 'nav_sessions', icon: ShieldCheck, title: 'Go to Sessions', meta: 'Navigation', to: '/settings/sessions', keywords: 'sessions devices' },
      { id: 'nav_trash', icon: Trash2, title: 'Go to Task trash', meta: 'Navigation', to: '/tasks-trash', keywords: 'trash deleted tasks' },
    ]
    const tasks: CommandEntry[] = (taskQuery.data?.pages.flatMap((page) => page.items) ?? []).map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id))).map((t) => ({
      id: t.id,
      icon: SquareCheck,
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
      className="fixed inset-0 z-[100] flex justify-center bg-black/40 p-4 backdrop-blur-[2px]"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="mt-[12vh] flex h-fit max-h-[min(60vh,28rem)] w-full max-w-[576px] flex-col overflow-hidden rounded-xl bg-card shadow-2xl ring-1 ring-border"
        onKeyDown={onKeyDown}
      >
        <div className="flex min-h-11 shrink-0 items-center gap-2.5 px-3.5 text-muted-foreground/70">
          <Search className="size-4 shrink-0" />
          <input
            ref={inputRef}
            className="h-11 flex-1 border-0 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground/70 focus:shadow-none focus:outline-none"
            placeholder="Search tasks and navigation…"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value)
              setActiveIndex(0)
            }}
          />
          <span className="inline-flex h-5 min-w-5 items-center justify-center rounded-md bg-accent px-1.5 text-[11px] font-medium text-muted-foreground/70">
            esc
          </span>
        </div>
        <div className="mx-1.5 mb-1.5 min-h-0 flex-1 overflow-y-auto rounded-lg bg-background p-1 ring-1 ring-border" ref={listRef}>
          {results.length === 0 ? (
            <div className="p-6 text-center text-[13px] text-muted-foreground">No results for “{query}”</div>
          ) : (
            results.map((entry, index) => (
              <button
                key={entry.id}
                className="relative flex h-10 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] text-foreground before:absolute before:top-2 before:bottom-2 before:left-0 before:w-0.5 before:rounded-full before:bg-primary before:opacity-0 data-[active=true]:bg-accent data-[active=true]:before:opacity-100"
                data-active={index === activeIndex}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => open(entry)}
              >
                <entry.icon className="size-4 shrink-0 text-muted-foreground/70" />
                <span className="truncate">{entry.title}</span>
                <span className="ml-auto text-[11px] whitespace-nowrap text-muted-foreground/70">{entry.meta}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
