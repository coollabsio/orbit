import { useEffect, useState } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router'
import { NavLink } from 'react-router'
import {
  Home2,
  Menu,
  Message,
  Moon,
  Note2,
  SearchNormal,
  Sms,
  Sun,
  TaskSquare,
} from 'reicon-react'
import { useTheme } from '../../lib/themeContext'
import { useAppState } from '../../mock/store'
import { Avatar } from '../ui/Avatar'
import { CommandPalette } from './CommandPalette'
import { SidebarNav, SidebarSettingsLink } from './SidebarNav'
import './shell.css'

const TITLES: Array<[string, string]> = [
  ['/tasks', 'Tasks'],
  ['/docs', 'Docs'],
  ['/mail', 'Mail'],
  ['/chat', 'Chat'],
  ['/inbox', 'Inbox'],
  ['/settings', 'Settings'],
]

const DOCK_LINKS = [
  { to: '/', label: 'Home', icon: Home2, end: true },
  { to: '/tasks', label: 'Tasks', icon: TaskSquare },
  { to: '/docs', label: 'Docs', icon: Note2 },
  { to: '/mail', label: 'Mail', icon: Sms },
  { to: '/chat', label: 'Chat', icon: Message },
]

export function AppShell() {
  const { theme, toggleTheme } = useTheme()
  const state = useAppState()
  const location = useLocation()
  const navigate = useNavigate()
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)

  const me = state.users.find((u) => u.id === state.currentUserId)
  const pageTitle = TITLES.find(([prefix]) => location.pathname.startsWith(prefix))?.[1] ?? 'Home'
  const unreadMail = state.mailThreads.filter((t) => t.unread && t.folderId === 'f_inbox').length
  const unreadChat = state.channels.reduce((sum, c) => sum + c.unreadCount, 0)
  const dockCounts: Record<string, number> = { '/mail': unreadMail, '/chat': unreadChat }

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPaletteOpen((o) => !o)
      }
    }
    const onOpen = () => setPaletteOpen(true)
    document.addEventListener('keydown', onKeyDown)
    window.addEventListener('open-command-palette', onOpen)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('open-command-palette', onOpen)
    }
  }, [])

  const themeButton = (
    <button className="icon-button" onClick={toggleTheme} aria-label="Toggle theme">
      {theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
    </button>
  )

  return (
    <div className="app-shell">
      <aside className="app-sidebar">
        <div className="app-sidebar-brand">
          <span className="app-sidebar-logo">O</span>
          <span className="app-sidebar-title">Orbit</span>
          {themeButton}
        </div>
        <SidebarNav />
        <div className="app-sidebar-footer">
          <SidebarSettingsLink />
          <button className="menu-item" onClick={() => navigate('/settings')}>
            <Avatar user={me} size={20} showOnline />
            <span className="menu-item-label">{me?.name}</span>
          </button>
        </div>
      </aside>

      <div className="app-content">
        <header className="mobile-topbar">
          <button className="icon-button" onClick={() => setDrawerOpen(true)} aria-label="Menu">
            <Menu size={18} />
          </button>
          <span className="mobile-topbar-title">{pageTitle}</span>
          <button className="icon-button" onClick={() => setPaletteOpen(true)} aria-label="Search">
            <SearchNormal size={18} />
          </button>
          {themeButton}
        </header>

        <Outlet />

        <nav className="mobile-dock">
          {DOCK_LINKS.map((link) => (
            <NavLink key={link.to} to={link.to} end={link.end}>
              {({ isActive }) => (
                <span className="mobile-dock-item" data-active={isActive}>
                  <link.icon size={20} />
                  {link.label}
                  {dockCounts[link.to] ? (
                    <span className="mobile-dock-badge">{dockCounts[link.to]}</span>
                  ) : null}
                </span>
              )}
            </NavLink>
          ))}
        </nav>
      </div>

      {drawerOpen ? (
        <>
          <div className="mobile-drawer-backdrop" onClick={() => setDrawerOpen(false)} />
          <aside className="mobile-drawer">
            <div className="app-sidebar-brand">
              <span className="app-sidebar-logo">O</span>
              <span className="app-sidebar-title">Orbit</span>
            </div>
            <SidebarNav onNavigate={() => setDrawerOpen(false)} />
            <div className="app-sidebar-footer">
              <SidebarSettingsLink onNavigate={() => setDrawerOpen(false)} />
            </div>
          </aside>
        </>
      ) : null}

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
    </div>
  )
}
