import { NavLink } from 'react-router'
import {
  DirectInbox,
  Home2,
  Message,
  Messages2,
  Note2,
  SearchNormal,
  Setting2,
  Sms,
  TaskSquare,
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
            className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
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
        <button type="button" className="menu-item menu-item-disabled" disabled title="Inbox — Coming soon">
          <DirectInbox size={18} />
          <span className="menu-item-label">Inbox</span>
          {!collapsed ? <span className="coming-soon">Coming soon</span> : null}
        </button>
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
