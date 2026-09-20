import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  Inbox,
  Home,
  Users,
  Settings,
  ShieldCheck,
  SquareCheck,
  Trash2,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '@/components/ui/command'
import { Kbd } from '@/components/ui/kbd'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjects } from '@/features/tasks/api/projects'
import { taskFromRecord } from '@/features/tasks/api/models'
import { useTasks } from '@/features/tasks/api/tasks'

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

  const open = (entry: CommandEntry | undefined) => {
    if (!entry) return
    onClose()
    navigate(entry.to)
  }

  return (
    <CommandDialog
      open
      onOpenChange={(next) => { if (!next) onClose() }}
      title="Command palette"
      description="Search tasks and navigation."
      className="top-[12vh] max-h-[min(60vh,28rem)] bg-card shadow-2xl ring-border sm:max-w-[576px]"
    >
      {/* `shouldFilter={false}`: the entry list is already filtered here (and tasks are
          filtered server-side), so cmdk only owns highlighting and keyboard navigation. */}
      <Command shouldFilter={false} className="min-h-0 bg-transparent p-0">
        <div className="relative shrink-0">
          <CommandInput
            autoFocus
            className="pr-9"
            placeholder="Search tasks and navigation…"
            value={query}
            onValueChange={setQuery}
          />
          <Kbd className="absolute top-1/2 right-3 -translate-y-1/2 px-1.5 text-[11px] text-muted-foreground/70">esc</Kbd>
        </div>
        <CommandList className="mx-1.5 mt-1 mb-1.5 max-h-none min-h-0 flex-1 rounded-lg bg-background p-1 ring-1 ring-border">
          <CommandEmpty className="p-6 text-[13px] text-muted-foreground">No results for “{query}”</CommandEmpty>
          <CommandGroup className="p-0">
            {results.map((entry) => (
              <CommandItem
                key={entry.id}
                value={entry.id}
                className="h-10 gap-2.5 px-2.5 text-[13px]"
                onSelect={() => open(entry)}
              >
                <entry.icon className="size-4 shrink-0 text-muted-foreground/70" />
                <span className="truncate">{entry.title}</span>
                <CommandShortcut className="text-[11px] tracking-normal whitespace-nowrap text-muted-foreground/70">{entry.meta}</CommandShortcut>
              </CommandItem>
            ))}
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
