import { useEffect, useLayoutEffect, useState } from 'react'
import { useWorkspaceEvents } from '../../features/realtime/useWorkspaceEvents'
import { Outlet } from 'react-router'
import { SidebarLeft } from 'reicon-react'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'
import { CommandPalette } from './CommandPalette'
import { SidebarNav } from './SidebarNav'
import { Topbar } from './Topbar'
import { UserMenu } from './UserMenu'
import './shell.css'
import { MobileDock } from './MobileDock'

export function AppShell() {
  const { workspace } = useWorkspace()
  const live = useWorkspaceEvents(workspace.id)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => window.localStorage.getItem('orbit:sidebar_collapsed') === 'true')

  useLayoutEffect(() => {
    // Safari can retain the document pan from the login keyboard after navigation.
    // The app scrolls within panes, so its outer document must start at the top.
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' })
    document.body.scrollTop = 0
  }, [])

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPaletteOpen((o) => !o)
      }
    }
    const onOpen = () => setPaletteOpen(true)
    const onOpenSidebar = () => setDrawerOpen(true)
    document.addEventListener('keydown', onKeyDown)
    window.addEventListener('open-command-palette', onOpen)
    window.addEventListener('open-sidebar', onOpenSidebar)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('open-command-palette', onOpen)
      window.removeEventListener('open-sidebar', onOpenSidebar)
    }
  }, [])

  useEffect(() => {
    window.localStorage.setItem('orbit:sidebar_collapsed', String(sidebarCollapsed))
  }, [sidebarCollapsed])

  return (
    <div className="app-shell">
      {!live ? <div className="connection-status" role="status">Connecting to live updates…</div> : null}
      <aside className="app-sidebar" data-collapsed={sidebarCollapsed || undefined}>
        <div className="app-sidebar-brand">
          <WorkspaceSwitcher collapsed={sidebarCollapsed} />
        </div>
        <SidebarNav collapsed={sidebarCollapsed} />
        <div className="app-sidebar-footer">
          <UserMenu collapsed={sidebarCollapsed} />
          <button
            type="button"
            className="icon-button app-sidebar-collapse"
            aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
          >
            <SidebarLeft size={17} />
          </button>
        </div>
      </aside>

      <div className="app-main">
        <Topbar onOpenDrawer={() => setDrawerOpen(true)} onOpenPalette={() => setPaletteOpen(true)} />
        <div className="app-content">
          <Outlet />
        </div>
        <MobileDock />
      </div>

      {drawerOpen ? (
        <>
          <div className="mobile-drawer-backdrop" onClick={() => setDrawerOpen(false)} />
          <aside className="mobile-drawer">
            <div className="app-sidebar-brand">
              <WorkspaceSwitcher onSelect={() => setDrawerOpen(false)} />
            </div>
            <SidebarNav onNavigate={() => setDrawerOpen(false)} />
            <div className="app-sidebar-footer">
              <UserMenu />
            </div>
          </aside>
        </>
      ) : null}

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
    </div>
  )
}
