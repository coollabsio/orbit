import { NavLink, useLocation } from 'react-router'
import { Calendar, DirectInbox as Inbox, Home2 as Home, Message as MessageSquare, Messages2 as MessagesSquare, DocumentText as FileText, SearchNormal as Search, Setting2 as Settings, Sms as Mail, TaskSquare as SquareCheck, Timer } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Kbd, KbdGroup } from '@/components/ui/kbd'
import { Separator } from '@/components/ui/separator'

const WORKSPACE_LINKS = [
  { to: '/tasks', label: 'Tasks', icon: SquareCheck, enabled: true },
  { to: '/', label: 'Home', icon: Home, enabled: false },
  { to: '/docs', label: 'Docs', icon: FileText, enabled: false },
  { to: '/mail', label: 'Mail', icon: Mail, enabled: false },
  { to: '/chat', label: 'Chat', icon: MessageSquare, enabled: false },
]

/** Grouped sidebar navigation — shared by the desktop sidebar and the mobile drawer. */
export function SidebarNav({ onNavigate, collapsed = false }: { onNavigate?: () => void; collapsed?: boolean }) {
  const location = useLocation()
  const currentParams = new URLSearchParams(location.search)
  const view = currentParams.get('view')
  const selectedProject = currentParams.get('project')

  const taskViewPath = (nextView?: string) => {
    const params = new URLSearchParams()
    if (selectedProject) params.set('project', selectedProject)
    if (nextView) params.set('view', nextView)
    return `/tasks${params.size > 0 ? `?${params}` : ''}`
  }

  const inboxPath = selectedProject ? `/inbox?project=${encodeURIComponent(selectedProject)}` : '/inbox'

  const itemClass = (active: boolean) =>
    cn(
      'relative flex h-8 w-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-2 text-left text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-accent-foreground',
      collapsed && 'justify-center px-0',
      active && 'bg-sidebar-accent text-sidebar-accent-foreground',
    )
  const labelClass = cn('min-w-0 flex-1 truncate', collapsed && 'hidden')
  const disabledClass = 'justify-start border-0 dark:hover:bg-sidebar-accent/50 disabled:pointer-events-auto disabled:cursor-not-allowed disabled:opacity-[0.48]'
  const comingSoonClass = 'ml-auto text-[10px] text-muted-foreground/70'

  const taskViewActive = (name: string | null) => location.pathname === '/tasks' && view === name

  const Section = ({ label, first }: { label: string; first?: boolean }) => {
    if (collapsed) {
      if (first) return null
      return <Separator className="mx-1 my-1.5 data-horizontal:w-auto" aria-hidden="true" />
    }
    return (
      <div
        className={cn(
          'px-2 pt-1 pb-[3px] text-[10px] leading-4 font-medium tracking-[0.02em] text-sidebar-foreground/60 select-none',
          !first && 'mt-2',
        )}
      >
        {label}
      </div>
    )
  }

  return (
    <>
      <Button
        variant="secondary"
        className={cn(
          'h-8 w-full justify-start gap-2 rounded-md border-0 bg-sidebar-accent px-2 text-[13px] font-medium text-muted-foreground/70 hover:bg-sidebar-accent/70 active:not-aria-[haspopup]:translate-y-0',
          collapsed && 'justify-center px-0',
        )}
        aria-label="Search"
        title={collapsed ? 'Search' : undefined}
        onClick={() => {
          onNavigate?.()
          window.dispatchEvent(new CustomEvent('open-command-palette'))
        }}
      >
        <Search className="size-[15px] shrink-0" />
        {!collapsed ? (
          <>
            Search{' '}
            <KbdGroup className="ml-auto">
              <Kbd className="rounded-md bg-sidebar-accent px-1.5 text-[11px] text-muted-foreground/70">⌘K</Kbd>
            </KbdGroup>
          </>
        ) : null}
      </Button>
      <div className={cn('mt-2 flex min-h-0 flex-1 flex-col overflow-y-auto', collapsed ? 'gap-1' : 'gap-0.5')}>
        <Section label="Workspace" first />
        {WORKSPACE_LINKS.map((link) => link.enabled ? (
          <NavLink
            key={link.to}
            to={link.to === '/tasks' ? taskViewPath() : link.to}
            className={({ isActive }) => itemClass(isActive && !view)}
            aria-label={link.label}
            title={collapsed ? link.label : undefined}
            onClick={onNavigate}
          >
            <link.icon className="size-[18px] shrink-0 opacity-90" />
            <span className={labelClass}>{link.label}</span>
          </NavLink>
        ) : (
          <Button key={link.to} variant="ghost" className={cn(disabledClass, itemClass(false))} disabled title={`${link.label} — Coming soon`}>
            <link.icon className="size-[18px] shrink-0 opacity-90" />
            <span className={labelClass}>{link.label}</span>
            {!collapsed ? <span className={comingSoonClass}>Coming soon</span> : null}
          </Button>
        ))}
        <Section label="Personal" />
        <NavLink
          to={inboxPath}
          className={({ isActive }) => itemClass(isActive)}
          aria-label="Inbox"
          title={collapsed ? 'Inbox' : undefined}
          onClick={onNavigate}
        >
          <Inbox className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Inbox</span>
        </NavLink>
        <NavLink
          to={taskViewPath('mine')}
          className={itemClass(taskViewActive('mine'))}
          aria-label="My tasks"
          title={collapsed ? 'My tasks' : undefined}
          onClick={onNavigate}
        >
          <SquareCheck className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>My tasks</span>
        </NavLink>
        <NavLink
          to={taskViewPath('current_week')}
          className={itemClass(taskViewActive('current_week'))}
          aria-label="This week"
          title={collapsed ? 'This week' : undefined}
          onClick={onNavigate}
        >
          <Calendar className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>This week</span>
        </NavLink>
        <NavLink
          to={taskViewPath('overdue')}
          className={itemClass(taskViewActive('overdue'))}
          aria-label="Overdue"
          title={collapsed ? 'Overdue' : undefined}
          onClick={onNavigate}
        >
          <Timer className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Overdue</span>
        </NavLink>
        <NavLink
          to={taskViewPath('due_soon')}
          className={itemClass(taskViewActive('due_soon'))}
          aria-label="Due soon"
          title={collapsed ? 'Due soon' : undefined}
          onClick={onNavigate}
        >
          <Calendar className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Due soon</span>
        </NavLink>
        <Button variant="ghost" className={cn(disabledClass, itemClass(false))} disabled title="Direct messages — Coming soon">
          <MessagesSquare className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Direct messages</span>
          {!collapsed ? <span className={comingSoonClass}>Coming soon</span> : null}
        </Button>
        <Section label="Manage" />
        <NavLink
          to="/settings"
          className={({ isActive }) => itemClass(isActive)}
          aria-label="Settings"
          title={collapsed ? 'Settings' : undefined}
          onClick={onNavigate}
        >
          <Settings className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Settings</span>
        </NavLink>
      </div>
    </>
  )
}
