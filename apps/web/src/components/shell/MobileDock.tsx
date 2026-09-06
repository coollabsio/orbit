import { NavLink } from 'react-router'
import { Home2, Message, Messages2, Note2, Setting2, Sms, TaskSquare } from 'reicon-react'
import { mobileDockPaths } from './productNavigation'

const DOCK_LINKS = [
  { to: '/', label: 'Home', icon: Home2 },
  { to: '/tasks', label: 'Tasks', icon: TaskSquare },
  { to: '/docs', label: 'Docs', icon: Note2 },
  { to: '/mail', label: 'Mail', icon: Sms },
  { to: '/chat', label: 'Chat', icon: Message },
  { to: '/dm', label: 'DMs', icon: Messages2 },
  { to: '/settings', label: 'Settings', icon: Setting2 },
]

export function MobileDock() {
  return (
    <nav className="mobile-dock" aria-label="Mobile navigation">
      {DOCK_LINKS.map((link) => mobileDockPaths.some((path) => path === link.to) ? (
        <NavLink key={link.to} to={link.to}>
          {({ isActive }) => (
            <span className="mobile-dock-item" data-active={isActive}>
              <link.icon size={20} />
              {link.label}
            </span>
          )}
        </NavLink>
      ) : (
        <button key={link.to} type="button" className="mobile-dock-item" disabled aria-label={`${link.label}, coming soon`}>
          <link.icon size={20} />
          {link.label}
          <span className="mobile-dock-soon">Soon</span>
        </button>
      ))}
    </nav>
  )
}
