import { useEffect, useState } from 'react'
import { NavLink, Outlet } from 'react-router'
import { Home2, Message, Note2, Sms, TaskSquare } from 'reicon-react'
import { useAppState } from '../../mock/store'
import { CommandPalette } from './CommandPalette'
import { SidebarNav } from './SidebarNav'
import { Topbar } from './Topbar'
import { UserMenu } from './UserMenu'
import './shell.css'

const DOCK_LINKS = [
  { to: '/', label: 'Home', icon: Home2, end: true },
  { to: '/tasks', label: 'Tasks', icon: TaskSquare },
  { to: '/docs', label: 'Docs', icon: Note2 },
  { to: '/mail', label: 'Mail', icon: Sms },
  { to: '/chat', label: 'Chat', icon: Message },
]

export function AppShell() {
  const state = useAppState()
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)

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

  return (
    <div className="app-shell">
      <aside className="app-sidebar">
        <div className="app-sidebar-brand">
          <span className="app-sidebar-title">Orbit</span>
          <span className="app-sidebar-version">v0.1.0</span>
        </div>
        <SidebarNav />
        <div className="app-sidebar-footer">
          <UserMenu />
        </div>
      </aside>

      <div className="app-main">
        <Topbar onOpenDrawer={() => setDrawerOpen(true)} onOpenPalette={() => setPaletteOpen(true)} />
        <div className="app-content">
          <Outlet />
        </div>
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
              <span className="app-sidebar-title">Orbit</span>
              <span className="app-sidebar-version">v0.1.0</span>
            </div>
            <SidebarNav onNavigate={() => setDrawerOpen(false)} />
          </aside>
        </>
      ) : null}

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
    </div>
  )
}
