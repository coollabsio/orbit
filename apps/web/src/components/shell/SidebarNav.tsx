import { NavLink, useNavigate } from 'react-router'
import {
  DirectInbox,
  Home2,
  Message,
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
export function SidebarNav({ onNavigate }: { onNavigate?: () => void }) {
  const state = useAppState()
  const navigate = useNavigate()
  const unreadNotifications = state.notifications.filter((n) => !n.readAt).length
  const unreadMail = state.mailThreads.filter((t) => t.unread && t.folderId === 'f_inbox').length
  const unreadChat = state.channels.reduce((sum, c) => sum + c.unreadCount, 0)

  const counts: Record<string, number> = {
    '/mail': unreadMail,
    '/chat': unreadChat,
  }

  return (
    <>
      <button
        className="sidebar-search"
        onClick={() => {
          onNavigate?.()
          window.dispatchEvent(new CustomEvent('open-command-palette'))
        }}
      >
        <SearchNormal size={15} />
        Search
        <span className="kbd">⌘K</span>
      </button>
      <div className="app-sidebar-scroll">
        <div className="nav-section">Workspace</div>
        {WORKSPACE_LINKS.map((link) => (
          <NavLink
            key={link.to}
            to={link.to}
            end={link.end}
            className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
            onClick={onNavigate}
          >
            <link.icon size={18} />
            <span className="menu-item-label">{link.label}</span>
            {counts[link.to] ? <span className="count-badge">{counts[link.to]}</span> : null}
          </NavLink>
        ))}
        <div className="nav-section">Personal</div>
        <NavLink
          to="/inbox"
          className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
          onClick={onNavigate}
        >
          <DirectInbox size={18} />
          <span className="menu-item-label">Inbox</span>
          {unreadNotifications ? <span className="count-badge">{unreadNotifications}</span> : null}
        </NavLink>
        <div className="nav-section">Projects</div>
        {state.projects.map((project) => (
          <button
            key={project.id}
            className="menu-item"
            onClick={() => {
              onNavigate?.()
              navigate(`/tasks?project=${project.id}`)
            }}
          >
            <span
              style={{
                width: 8,
                height: 8,
                borderRadius: 2,
                background: project.color,
                marginInline: 5,
                flexShrink: 0,
              }}
            />
            <span className="menu-item-label">{project.name}</span>
          </button>
        ))}
        <div className="nav-section">Manage</div>
        <NavLink
          to="/settings"
          className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
          onClick={onNavigate}
        >
          <Setting2 size={18} />
          <span className="menu-item-label">Settings</span>
        </NavLink>
      </div>
    </>
  )
}
