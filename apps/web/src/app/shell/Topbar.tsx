import { Link, useLocation, useNavigate, useSearchParams } from 'react-router'
import { Add as Plus, Menu, Moon, SearchNormal as Search, Setting2 as Settings, Sun } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { useTheme } from '@/lib/themeContext'
import { useAppState } from '@/mock/store'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { taskFromRecord, type Project, type Task, type TaskStatusDef } from '@/features/tasks/api/models'
import { useTasks } from '@/features/tasks/api/tasks'
import { threadTitleOf } from '@/lib/messagePreview'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import type { AppState } from '@/mock/types'

interface Crumb {
  label: string
  to?: string
}

type TaskNavigation = { projects: Project[]; tasks: Task[]; statuses: TaskStatusDef[] }

function crumbsFor(pathname: string, folderParam: string | null, state: AppState, taskNavigation: TaskNavigation): { crumbs: Crumb[]; status?: React.ReactNode } {
  const [, root, id, sub, subId] = pathname.split('/')
  switch (root) {
    case 'tasks': {
      const crumbs: Crumb[] = [{ label: 'Tasks', to: '/tasks' }]
      if (id === 'projects' && subId === 'settings') {
        const project = taskNavigation.projects.find((p) => p.id === sub)
        if (project) crumbs.push({ label: project.name, to: `/tasks?project=${project.id}` })
        crumbs.push({ label: 'Settings' })
        return { crumbs }
      }
      const task = id ? taskNavigation.tasks.find((t) => t.id === id) : null
      if (task) {
        const status = taskNavigation.statuses.find((s) => s.id === task.statusId)
        const project = taskNavigation.projects.find((p) => p.id === task.projectId)
        if (project) crumbs.push({ label: project.name, to: `/tasks?project=${project.id}` })
        crumbs.push({ label: task.identifier })
        return {
          crumbs,
          status: (
            <span className="inline-flex h-[26px] shrink-0 items-center gap-1.5 rounded-full border border-border bg-card px-2.5 text-xs font-medium text-muted-foreground max-[899px]:hidden">
              <TaskStatusIcon status={status} size={12} />
              {status?.name ?? 'No status'}
            </span>
          ),
        }
      }
      return { crumbs }
    }
    case 'docs': {
      const crumbs: Crumb[] = [{ label: 'Docs', to: '/docs' }]
      let doc = id ? state.docs.find((d) => d.id === id) : null
      const chain: Crumb[] = []
      while (doc) {
        chain.unshift({ label: doc.title, to: `/docs/${doc.id}` })
        doc = doc.parentId ? state.docs.find((d) => d.id === doc?.parentId) : null
      }
      return { crumbs: [...crumbs, ...chain] }
    }
    case 'mail': {
      const crumbs: Crumb[] = [{ label: 'Mail', to: '/mail' }]
      const thread = id ? state.mailThreads.find((t) => t.id === id) : null
      const folderId = folderParam ?? thread?.folderId ?? 'f_inbox'
      const folder = state.mailFolders.find((f) => f.id === folderId)
      if (folder) crumbs.push({ label: folder.name, to: `/mail?folder=${folder.id}` })
      if (thread) crumbs.push({ label: thread.subject })
      return { crumbs }
    }
    case 'chat': {
      const crumbs: Crumb[] = [{ label: 'Chat', to: '/chat' }]
      const channel = id ? state.channels.find((c) => c.id === id) : null
      const threadRoot = channel && sub === 'thread' && subId ? state.chatMessages.find((m) => m.id === subId) : null
      if (id === 'settings') crumbs.push({ label: 'Chat Settings' })
      else if (channel && threadRoot) {
        crumbs.push({ label: `#${channel.name}`, to: `/chat/${channel.id}` }, { label: threadTitleOf(threadRoot) })
      } else if (channel) crumbs.push({ label: `#${channel.name}` })
      return { crumbs }
    }
    case 'dm': {
      const crumbs: Crumb[] = [{ label: 'Direct Messages', to: '/dm' }]
      const dm = id ? state.directMessages.find((candidate) => candidate.id === id) : null
      const participant = dm ? state.users.find((user) => user.id === dm.participantId) : null
      if (participant) crumbs.push({ label: participant.name })
      return { crumbs }
    }
    case 'inbox':
      return { crumbs: [{ label: 'Inbox' }] }
    case 'profile':
      return { crumbs: [{ label: 'Profile' }] }
    case 'settings': {
      const crumbs: Crumb[] = [{ label: 'Settings', to: '/settings' }]
      if (id === 'members') crumbs.push({ label: 'Members' })
      else if (id === 'github') crumbs.push({ label: 'GitHub' })
      else if (id === 'sessions') crumbs.push({ label: 'Sessions' })
      else if (id === 'danger-zone') crumbs.push({ label: 'Danger zone' })
      return { crumbs }
    }
    default:
      return { crumbs: [{ label: 'Home' }] }
  }
}

export function Topbar({ onOpenDrawer, onOpenPalette }: { onOpenDrawer: () => void; onOpenPalette: () => void }) {
  const state = useAppState()
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id)
  const statuses = useAllStatuses(workspace.id, projects.data ?? [])
  const tasks = useTasks(workspace.id, { limit: 100 })
  const location = useLocation()
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const { theme, toggleTheme } = useTheme()

  const taskNavigation = {
    projects: projects.data ?? [],
    statuses: statuses.data,
    tasks: tasks.data?.pages.flatMap((page) => page.items.map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id)))) ?? [],
  }
  const { crumbs, status } = crumbsFor(location.pathname, searchParams.get('folder'), state, taskNavigation)
  const routeRoot = location.pathname.split('/')[1] || 'home'

  const crumbBase =
    'min-w-0 shrink overflow-hidden text-[13.5px] font-medium whitespace-nowrap text-ellipsis text-muted-foreground group-data-[root=home]/topbar:text-sm group-data-[root=home]/topbar:font-semibold group-data-[root=home]/topbar:text-foreground group-data-[root=settings]/topbar:text-[13px] group-data-[root=settings]/topbar:font-semibold'
  const menuOptionClass =
    'min-h-8 w-full gap-2 rounded-md px-2 py-1.5 text-left font-normal text-foreground focus:bg-accent focus:text-accent-foreground data-disabled:opacity-40'

  return (
    <header
      className={cn(
        'group/topbar hidden h-12 shrink-0 items-center gap-2 bg-background px-2 max-[899px]:flex',
        'data-[root=settings]:h-11 data-[root=settings]:min-h-11 data-[root=settings]:gap-[7px] data-[root=settings]:border-b data-[root=settings]:border-border data-[root=settings]:px-2 data-[root=settings]:py-1.5',
        'data-[root=tasks]:hidden data-[root=mail]:hidden data-[root=docs]:hidden data-[root=dm]:hidden data-[root=chat]:hidden data-[root=inbox]:hidden',
      )}
      data-root={routeRoot}
    >
      <Button
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground/70 group-data-[root=home]/topbar:hidden"
        onClick={onOpenDrawer}
        aria-label="Menu"
      >
        <Menu className="size-[18px]" />
      </Button>
      <nav className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden whitespace-nowrap group-data-[root=home]/topbar:gap-0 group-data-[root=settings]/topbar:gap-[7px] group-data-[root=settings]/topbar:py-[5px]">
        {crumbs.map((crumb, index) => {
          const isLast = index === crumbs.length - 1
          return (
            <span key={index} className="contents">
              {index > 0 ? <span className="shrink-0 text-[13px] text-muted-foreground/70 select-none">/</span> : null}
              {crumb.to && !isLast ? (
                <Link className={cn(crumbBase, 'hover:text-foreground')} to={crumb.to}>
                  {crumb.label}
                </Link>
              ) : (
                <span className={cn(crumbBase, 'data-[current=true]:max-w-[40vw] data-[current=true]:shrink-0 data-[current=true]:text-foreground')} data-current={isLast}>
                  {crumb.label}
                </span>
              )}
            </span>
          )
        })}
        {status}
      </nav>
      <div className="flex shrink-0 items-center gap-1.5 group-data-[root=home]/topbar:gap-0.5 group-data-[root=settings]/topbar:gap-2">
        <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={onOpenPalette} aria-label="Search">
          <Search className={routeRoot === 'settings' ? 'size-[15px]' : 'size-4'} />
        </Button>
        <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={toggleTheme} aria-label="Toggle theme">
          {theme === 'dark' ? <Sun className="size-[15px]" /> : <Moon className="size-[15px]" />}
        </Button>
        {routeRoot === 'home' ? (
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate('/settings')} aria-label="Settings">
            <Settings className="size-[15px]" />
          </Button>
        ) : null}
        {routeRoot !== 'home' && routeRoot !== 'settings' && routeRoot !== 'profile' ? (
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button />}>
              <Plus className="size-4" />
              New
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[160px] p-1">
              <DropdownMenuItem className={menuOptionClass} onClick={() => navigate('/tasks?new=1')}>
                Task
              </DropdownMenuItem>
              <DropdownMenuItem className={menuOptionClass} disabled>Document</DropdownMenuItem>
              <DropdownMenuItem className={menuOptionClass} disabled>Email</DropdownMenuItem>
              <DropdownMenuItem className={menuOptionClass} disabled>Chat message</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
      </div>
    </header>
  )
}
