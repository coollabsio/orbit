import { NavLink, useLocation } from 'react-router'
import {
  Calendar,
  DirectInbox,
  Home2,
  Message,
  Messages2,
  Note2,
  SearchNormal,
  Setting2,
  Sms,
  TaskSquare,
  Timer,
} from 'reicon-react'
const WORKSPACE_LINKS = [
  { to: '/tasks', label: 'Tasks', icon: TaskSquare, enabled: true },
  { to: '/', label: 'Home', icon: Home2, enabled: false },
  { to: '/docs', label: 'Docs', icon: Note2, enabled: false },
  { to: '/mail', label: 'Mail', icon: Sms, enabled: false },
  { to: '/chat', label: 'Chat', icon: Message, enabled: false },
]

/** Grouped sidebar navigation — shared by the desktop sidebar and the mobile drawer. */
export function SidebarNav({ onNavigate, collapsed = false }: { onNavigate?: () => void; collapsed?: boolean }) {
  const location = useLocation()
  const view = new URLSearchParams(location.search).get('view')
  const taskViewClass = (name: string | null) =>
    location.pathname === '/tasks' && view === name ? 'menu-item active' : 'menu-item'
  return (
    <>
      <button
        className="sidebar-search"
        aria-label="Search"
        title={collapsed ? 'Search' : undefined}
        onClick={() => {
          onNavigate?.()
          window.dispatchEvent(new CustomEvent('open-command-palette'))
        }}
      >
        <SearchNormal size={15} />
        {!collapsed ? <>Search <span className="kbd">⌘K</span></> : null}
      </button>
      <div className="app-sidebar-scroll">
        <div className="nav-section">Workspace</div>
        {WORKSPACE_LINKS.map((link) => link.enabled ? (
          <NavLink
            key={link.to}
            to={link.to}
            className={({ isActive }) => (isActive && !view ? 'menu-item active' : 'menu-item')}
            aria-label={link.label}
            title={collapsed ? link.label : undefined}
            onClick={onNavigate}
          >
            <link.icon size={18} />
            <span className="menu-item-label">{link.label}</span>
          </NavLink>
        ) : (
          <button key={link.to} type="button" className="menu-item menu-item-disabled" disabled title={`${link.label} — Coming soon`}>
            <link.icon size={18} />
            <span className="menu-item-label">{link.label}</span>
            {!collapsed ? <span className="coming-soon">Coming soon</span> : null}
          </button>
        ))}
        <div className="nav-section">Personal</div>
        <NavLink
          to="/inbox"
          className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
          aria-label="Inbox"
          title={collapsed ? 'Inbox' : undefined}
          onClick={onNavigate}
        >
          <DirectInbox size={18} />
          <span className="menu-item-label">Inbox</span>
        </NavLink>
        <NavLink
          to="/tasks?view=mine"
          className={() => taskViewClass('mine')}
          aria-label="My tasks"
          title={collapsed ? 'My tasks' : undefined}
          onClick={onNavigate}
        >
          <TaskSquare size={18} />
          <span className="menu-item-label">My tasks</span>
        </NavLink>
        <NavLink
          to="/tasks?view=current_week"
          className={() => taskViewClass('current_week')}
          aria-label="This week"
          title={collapsed ? 'This week' : undefined}
          onClick={onNavigate}
        >
          <Calendar size={18} />
          <span className="menu-item-label">This week</span>
        </NavLink>
        <NavLink
          to="/tasks?view=overdue"
          className={() => taskViewClass('overdue')}
          aria-label="Overdue"
          title={collapsed ? 'Overdue' : undefined}
          onClick={onNavigate}
        >
          <Timer size={18} />
          <span className="menu-item-label">Overdue</span>
        </NavLink>
        <NavLink
          to="/tasks?view=due_soon"
          className={() => taskViewClass('due_soon')}
          aria-label="Due soon"
          title={collapsed ? 'Due soon' : undefined}
          onClick={onNavigate}
        >
          <Calendar size={18} />
          <span className="menu-item-label">Due soon</span>
        </NavLink>
        <button type="button" className="menu-item menu-item-disabled" disabled title="Direct messages — Coming soon">
          <Messages2 size={18} />
          <span className="menu-item-label">Direct messages</span>
          {!collapsed ? <span className="coming-soon">Coming soon</span> : null}
        </button>
        <div className="nav-section">Manage</div>
        <NavLink
          to="/settings"
          className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
          aria-label="Settings"
          title={collapsed ? 'Settings' : undefined}
          onClick={onNavigate}
        >
          <Setting2 size={18} />
          <span className="menu-item-label">Settings</span>
        </NavLink>
      </div>
    </>
  )
}
