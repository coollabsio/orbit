import { NavLink, useLocation } from 'react-router'
import {
  Calendar,
  Inbox,
  Home,
  MessageSquare,
  MessagesSquare,
  FileText,
  Search,
  Settings,
  Mail,
  SquareCheck,
  Timer,
} from 'lucide-react'
import { cn } from 'cn'

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
  const view = new URLSearchParams(location.search).get('view')

  const itemClass = (active: boolean) =>
    cn(
      'relative flex h-8 w-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-2 text-left text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-accent-foreground',
      collapsed && 'justify-center px-0',
      active && 'bg-sidebar-accent text-sidebar-accent-foreground',
    )
  const labelClass = cn('min-w-0 flex-1 truncate', collapsed && 'hidden')
  const disabledClass = 'cursor-not-allowed opacity-[0.48]'
  const comingSoonClass = 'ml-auto text-[10px] text-muted-foreground/70'

  const taskViewActive = (name: string | null) => location.pathname === '/tasks' && view === name

  const Section = ({ label, first }: { label: string; first?: boolean }) => {
    if (collapsed) {
      if (first) return null
      return <div className="mx-1 my-1.5 h-px bg-border" aria-hidden="true" />
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
      <button
        className={cn(
          'flex h-8 w-full shrink-0 items-center gap-2 rounded-md bg-sidebar-accent px-2 text-[13px] font-medium text-muted-foreground/70 transition-colors hover:bg-sidebar-accent/70',
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
            <span className="ml-auto inline-flex h-5 min-w-5 items-center justify-center rounded-md bg-sidebar-accent px-1.5 text-[11px] font-medium text-muted-foreground/70">
              ⌘K
            </span>
          </>
        ) : null}
      </button>
      <div className={cn('mt-2 flex min-h-0 flex-1 flex-col overflow-y-auto', collapsed ? 'gap-1' : 'gap-0.5')}>
        <Section label="Workspace" first />
        {WORKSPACE_LINKS.map((link) => link.enabled ? (
          <NavLink
            key={link.to}
            to={link.to}
            className={({ isActive }) => itemClass(isActive && !view)}
            aria-label={link.label}
            title={collapsed ? link.label : undefined}
            onClick={onNavigate}
          >
            <link.icon className="size-[18px] shrink-0 opacity-90" />
            <span className={labelClass}>{link.label}</span>
          </NavLink>
        ) : (
          <button key={link.to} type="button" className={cn(itemClass(false), disabledClass)} disabled title={`${link.label} — Coming soon`}>
            <link.icon className="size-[18px] shrink-0 opacity-90" />
            <span className={labelClass}>{link.label}</span>
            {!collapsed ? <span className={comingSoonClass}>Coming soon</span> : null}
          </button>
        ))}
        <Section label="Personal" />
        <NavLink
          to="/inbox"
          className={({ isActive }) => itemClass(isActive)}
          aria-label="Inbox"
          title={collapsed ? 'Inbox' : undefined}
          onClick={onNavigate}
        >
          <Inbox className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Inbox</span>
        </NavLink>
        <NavLink
          to="/tasks?view=mine"
          className={itemClass(taskViewActive('mine'))}
          aria-label="My tasks"
          title={collapsed ? 'My tasks' : undefined}
          onClick={onNavigate}
        >
          <SquareCheck className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>My tasks</span>
        </NavLink>
        <NavLink
          to="/tasks?view=current_week"
          className={itemClass(taskViewActive('current_week'))}
          aria-label="This week"
          title={collapsed ? 'This week' : undefined}
          onClick={onNavigate}
        >
          <Calendar className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>This week</span>
        </NavLink>
        <NavLink
          to="/tasks?view=overdue"
          className={itemClass(taskViewActive('overdue'))}
          aria-label="Overdue"
          title={collapsed ? 'Overdue' : undefined}
          onClick={onNavigate}
        >
          <Timer className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Overdue</span>
        </NavLink>
        <NavLink
          to="/tasks?view=due_soon"
          className={itemClass(taskViewActive('due_soon'))}
          aria-label="Due soon"
          title={collapsed ? 'Due soon' : undefined}
          onClick={onNavigate}
        >
          <Calendar className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Due soon</span>
        </NavLink>
        <button type="button" className={cn(itemClass(false), disabledClass)} disabled title="Direct messages — Coming soon">
          <MessagesSquare className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Direct messages</span>
          {!collapsed ? <span className={comingSoonClass}>Coming soon</span> : null}
        </button>
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
