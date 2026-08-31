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
import { useAppState } from '../../mock/store'

const WORKSPACE_LINKS = [
  { to: '/', label: 'Home', icon: Home2, end: true },
  { to: '/tasks', label: 'Tasks', icon: TaskSquare },
  { to: '/docs', label: 'Docs', icon: Note2 },
  { to: '/mail', label: 'Mail', icon: Sms },
  { to: '/chat', label: 'Chat', icon: Message },
]

/** Grouped sidebar navigation — shared by the desktop sidebar and the mobile drawer. */
export function SidebarNav({ onNavigate, collapsed = false }: { onNavigate?: () => void; collapsed?: boolean }) {
  const state = useAppState()
  const unreadNotifications = state.notifications.filter((n) => !n.readAt).length
  const unreadMail = state.mailThreads.filter((t) => t.unread && t.folderId === 'f_inbox').length
  const unreadChat = state.channels.reduce((sum, c) => sum + c.unreadCount, 0)
  const unreadDMs = state.directMessages.reduce((sum, dm) => sum + dm.unreadCount, 0)

  const counts: Record<string, number> = {
    '/mail': unreadMail,
    '/chat': unreadChat,
    '/dm': unreadDMs,
  }

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
        {WORKSPACE_LINKS.map((link) => (
          <NavLink
            key={link.to}
            to={link.to}
            end={link.end}
            className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
            aria-label={link.label}
            title={collapsed ? link.label : undefined}
            onClick={onNavigate}
          >
            <link.icon size={18} />
            <span className="menu-item-label">{link.label}</span>
            {counts[link.to] ? <span className="count-badge">{counts[link.to]}</span> : null}
          </NavLink>
        ))}
        <div className="nav-section">Personal</div>
        <NavLink
          to="/dm"
          className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
          aria-label="Direct messages"
          title={collapsed ? 'Direct messages' : undefined}
          onClick={onNavigate}
        >
          <Messages2 size={18} />
          <span className="menu-item-label">Direct messages</span>
          {unreadDMs ? <span className="count-badge">{unreadDMs}</span> : null}
        </NavLink>
        <NavLink
          to="/inbox"
          className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
          aria-label="Inbox"
          title={collapsed ? 'Inbox' : undefined}
          onClick={onNavigate}
        >
          <DirectInbox size={18} />
          <span className="menu-item-label">Inbox</span>
          {unreadNotifications ? <span className="count-badge">{unreadNotifications}</span> : null}
        </NavLink>
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
