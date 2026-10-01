import { Shortcut } from '@/shortcuts/Shortcut'
import { createContext, Fragment, useContext, type ComponentType, type ReactNode } from 'react'
import { Link, useLocation, useMatch, useResolvedPath, type LinkProps } from 'react-router'
import { cva, type VariantProps } from 'class-variance-authority'
import { Calendar, DirectInbox as Inbox, Home2 as Home, Layer, Message as MessageSquare, Messages2 as MessagesSquare, DocumentText as FileText, SearchNormal as Search, Setting2 as Settings, Sms as Mail, TaskSquare as SquareCheck, Timer } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { docsHidden } from './productNavigation'

const WORKSPACE_LINKS = [
  { to: '/tasks', label: 'Tasks', icon: SquareCheck, enabled: true },
  { to: '/', label: 'Home', icon: Home, enabled: false },
  { to: '/docs', label: 'Docs', icon: FileText, enabled: true },
  { to: '/mail', label: 'Mail', icon: Mail, enabled: false },
  { to: '/chat', label: 'Chat', icon: MessageSquare, enabled: false },
].filter((link) => !(docsHidden && link.to === '/docs'))

const sidebarNavItemVariants = cva(
  'relative flex h-8 w-full min-w-0 shrink-0 items-center justify-start gap-2 overflow-hidden rounded-md px-2 text-left text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground group-data-[collapsed=true]/sidebar-nav:justify-center group-data-[collapsed=true]/sidebar-nav:px-0 [&_svg]:size-[18px] [&_svg]:shrink-0 [&_svg]:opacity-90',
  {
    variants: {
      size: {
        default: '',
        // nested under its parent entry while expanded; a plain rail icon while collapsed
        sub: 'group-data-[collapsed=false]/sidebar-nav:h-7 group-data-[collapsed=false]/sidebar-nav:pl-[34px] group-data-[collapsed=false]/sidebar-nav:font-normal group-data-[collapsed=false]/sidebar-nav:[&_svg]:size-4',
      },
    },
    defaultVariants: { size: 'default' },
  },
)

const SidebarNavCollapsed = createContext(false)

type SidebarNavIcon = ComponentType<{ className?: string }>

function SidebarNavLabel({ children }: { children: ReactNode }) {
  return <span data-slot="sidebar-nav-label" className="min-w-0 flex-1 truncate group-data-[collapsed=true]/sidebar-nav:hidden">{children}</span>
}

/** A sidebar link. Active when the route matches `to` (like NavLink), unless `active` says otherwise. */
function SidebarNavItem({
  to,
  end = false,
  icon: Icon,
  label,
  'aria-label': ariaLabel = label,
  active,
  size,
  className,
  ...props
}: Omit<LinkProps, 'className' | 'children' | 'title'> & VariantProps<typeof sidebarNavItemVariants> & {
  className?: string
  end?: boolean
  icon: SidebarNavIcon
  label: string
  active?: boolean
}) {
  const collapsed = useContext(SidebarNavCollapsed)
  const match = useMatch({ path: useResolvedPath(to).pathname, end })
  const isActive = active ?? match !== null
  return (
    <Link
      data-slot="sidebar-nav-item"
      data-active={isActive}
      aria-current={isActive ? 'page' : undefined}
      aria-label={ariaLabel}
      title={collapsed ? ariaLabel : undefined}
      className={cn(sidebarNavItemVariants({ size }), className)}
      to={to}
      {...props}
    >
      <Icon />
      <SidebarNavLabel>{label}</SidebarNavLabel>
    </Link>
  )
}

/** A product area that is not built yet: shown, but disabled. */
function SidebarNavComingSoon({ icon: Icon, label }: { icon: SidebarNavIcon; label: string }) {
  return (
    <Button
      data-slot="sidebar-nav-item"
      variant="ghost"
      className={cn(sidebarNavItemVariants(), 'border-0 dark:hover:bg-sidebar-accent/50 disabled:pointer-events-auto disabled:cursor-not-allowed disabled:opacity-[0.48]')}
      disabled
      title={`${label} — Coming soon`}
    >
      {/* an explicit size: Button's own svg rule outranks the item's [&_svg] one */}
      <Icon className="size-[18px]" />
      <SidebarNavLabel>{label}</SidebarNavLabel>
      <span className="ml-auto text-[10px] text-muted-foreground/70 group-data-[collapsed=true]/sidebar-nav:hidden">Coming soon</span>
    </Button>
  )
}

function SidebarSection({ label, collapsed, first }: { label: string; collapsed: boolean; first?: boolean }) {
  if (collapsed) {
    if (first) return null
    return <Separator className="mx-1 my-1.5 data-horizontal:w-auto" aria-hidden="true" />
  }
  return (
    <div className={cn('px-2 pt-1 pb-[3px] text-[10px] leading-4 font-medium tracking-[0.02em] text-sidebar-foreground/60 select-none', !first && 'mt-2')}>
      {label}
    </div>
  )
}

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

  // Tasks itself is active on its routes only while no task view is picked
  const tasksActive = useMatch({ path: '/tasks', end: false }) !== null && !view
  const taskViewActive = (name: string) => location.pathname === '/tasks' && view === name
  // Views is a part of Tasks: its entry shows only while the user is somewhere in Tasks
  const inTasks = /^\/(tasks|views)(\/|-|$)/.test(location.pathname)

  return (
    <SidebarNavCollapsed.Provider value={collapsed}>
      <div data-slot="sidebar-nav" data-collapsed={collapsed} className="group/sidebar-nav flex min-h-0 flex-1 flex-col">
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
              <Shortcut id="palette.open" className="ml-auto **:data-[slot=kbd]:rounded-md **:data-[slot=kbd]:bg-sidebar-accent **:data-[slot=kbd]:text-[11px] **:data-[slot=kbd]:text-muted-foreground/70" />
            </>
          ) : null}
        </Button>
        <div className={cn('mt-2 flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain', collapsed ? 'gap-1' : 'gap-0.5')}>
          <SidebarSection label="Workspace" collapsed={collapsed} first />
          {WORKSPACE_LINKS.map((link) => link.enabled ? (
            <Fragment key={link.to}>
              <SidebarNavItem
                to={link.to === '/tasks' ? taskViewPath() : link.to}
                active={link.to === '/tasks' ? tasksActive : undefined}
                icon={link.icon}
                label={link.label}
                onClick={onNavigate}
              />
              {/* saved views belong to Tasks, so they nest under it instead of sitting beside the other apps */}
              {link.to === '/tasks' && inTasks ? (
                <SidebarNavItem to="/views" end size="sub" icon={Layer} label="Views" aria-label="Task views" className="animate-relation-enter motion-reduce:animate-none" onClick={onNavigate} />
              ) : null}
            </Fragment>
          ) : (
            <SidebarNavComingSoon key={link.to} icon={link.icon} label={link.label} />
          ))}
          <SidebarSection label="Personal" collapsed={collapsed} />
          <SidebarNavItem to={inboxPath} icon={Inbox} label="Inbox" onClick={onNavigate} />
          <SidebarNavItem to={taskViewPath('mine')} active={taskViewActive('mine')} icon={SquareCheck} label="My tasks" onClick={onNavigate} />
          <SidebarNavItem to={taskViewPath('current_week')} active={taskViewActive('current_week')} icon={Calendar} label="This week" onClick={onNavigate} />
          <SidebarNavItem to={taskViewPath('my_week')} active={taskViewActive('my_week')} icon={Calendar} label="My week" onClick={onNavigate} />
          <SidebarNavItem to={taskViewPath('overdue')} active={taskViewActive('overdue')} icon={Timer} label="Overdue" onClick={onNavigate} />
          <SidebarNavItem to={taskViewPath('due_soon')} active={taskViewActive('due_soon')} icon={Calendar} label="Due soon" onClick={onNavigate} />
          <SidebarNavComingSoon icon={MessagesSquare} label="Direct messages" />
          <SidebarSection label="Manage" collapsed={collapsed} />
          <SidebarNavItem to="/settings" icon={Settings} label="Settings" onClick={onNavigate} />
        </div>
      </div>
    </SidebarNavCollapsed.Provider>
  )
}
