import { useEffect, useState } from 'react'
import { useWorkspaceEvents } from '../../features/realtime/useWorkspaceEvents'
import { Outlet } from 'react-router'
import { ChevronDown, SidebarLeft } from 'reicon-react'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { Dropdown } from '../ui/Dropdown'
import { CommandPalette } from './CommandPalette'
import { SidebarNav } from './SidebarNav'
import { Topbar } from './Topbar'
import { UserMenu } from './UserMenu'
import './shell.css'
import { MobileDock } from './MobileDock'

export function AppShell() {
  const { workspace, workspaces, selectWorkspace } = useWorkspace()
  const live = useWorkspaceEvents(workspace.id)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => window.localStorage.getItem('orbit:sidebar_collapsed') === 'true')

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

  useEffect(() => {
    window.localStorage.setItem('orbit:sidebar_collapsed', String(sidebarCollapsed))
  }, [sidebarCollapsed])

  return (
    <div className="app-shell">
      {!live ? <div className="connection-status" role="status">Connecting to live updates…</div> : null}
      <aside className="app-sidebar" data-collapsed={sidebarCollapsed || undefined}>
        <div className="app-sidebar-brand">
          <Dropdown trigger={() => <button className="app-sidebar-wordmark" aria-label={`Workspace: ${workspace.name}`}>
            <span className="app-sidebar-title">{sidebarCollapsed ? workspace.name.charAt(0) : workspace.name}</span>
            {!sidebarCollapsed ? <ChevronDown size={13} /> : null}
          </button>}>
            {(close) => <>{workspaces.map((item) => <button key={item.id} className="popover-option" data-selected={item.id === workspace.id || undefined} onClick={() => { selectWorkspace(item.id); close() }}>{item.name}</button>)}</>}
          </Dropdown>
        </div>
        <SidebarNav collapsed={sidebarCollapsed} />
        <div className="app-sidebar-footer">
          <UserMenu collapsed={sidebarCollapsed} />
          <span className="spacer" />
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
