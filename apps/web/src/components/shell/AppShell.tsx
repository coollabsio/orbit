import { useCallback, useEffect, useLayoutEffect, useState } from 'react'
import { useWorkspaceEvents } from '../../features/realtime/useWorkspaceEvents'
import { Outlet, useNavigate } from 'react-router'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { CommandPalette } from './CommandPalette'
import { SidebarBrand } from './SidebarBrand'
import { SidebarNav } from './SidebarNav'
import { useNewTaskShortcut } from './newTaskShortcut'
import { useSidebarToggleShortcut } from './sidebarShortcut'
import { Topbar } from './Topbar'
import { TopbarSlotProvider } from './TopbarSlot'
import { UserMenu } from './UserMenu'
import './shell.css'
import { MobileDock } from './MobileDock'

export function AppShell() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const live = useWorkspaceEvents(workspace.id)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => window.localStorage.getItem('orbit:sidebar_collapsed') === 'true')

  useLayoutEffect(() => {
    const viewport = window.visualViewport
    const resetDocumentScroll = () => {
      // The shell scrolls inside panes. Safari can also pan the outer document
      // when any input opens the keyboard, even after the shell has mounted.
      // Leave panning alone while the user is pinch-zoomed.
      if (viewport && viewport.scale > 1) return
      if (window.scrollX || window.scrollY || document.body.scrollTop || viewport?.offsetTop) {
        window.scrollTo({ top: 0, left: 0, behavior: 'instant' })
        document.body.scrollTop = 0
      }
    }
    resetDocumentScroll()
    window.addEventListener('scroll', resetDocumentScroll)
    viewport?.addEventListener('resize', resetDocumentScroll)
    viewport?.addEventListener('scroll', resetDocumentScroll)
    return () => {
      window.removeEventListener('scroll', resetDocumentScroll)
      viewport?.removeEventListener('resize', resetDocumentScroll)
      viewport?.removeEventListener('scroll', resetDocumentScroll)
    }
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

  const toggleSidebar = useCallback(() => setSidebarCollapsed((collapsed) => !collapsed), [])
  useSidebarToggleShortcut(toggleSidebar)

  // Replaces the deleted topbar New dropdown: creating a task stays one keystroke from any route.
  const newTask = useCallback(() => navigate('/tasks?new=1'), [navigate])
  useNewTaskShortcut(newTask)

  return (
    <div className="app-shell">
      {!live ? <div className="connection-status" role="status">Connecting to live updates…</div> : null}
      <aside className="app-sidebar" data-collapsed={sidebarCollapsed || undefined}>
        <SidebarBrand collapsed={sidebarCollapsed} onToggleCollapse={toggleSidebar} />
        <SidebarNav collapsed={sidebarCollapsed} />
        <div className="app-sidebar-footer">
          <UserMenu collapsed={sidebarCollapsed} />
        </div>
      </aside>

      <div className="app-main">
        <TopbarSlotProvider>
          <Topbar onOpenDrawer={() => setDrawerOpen(true)} />
          <div className="app-content">
            <Outlet />
          </div>
        </TopbarSlotProvider>
        <MobileDock />
      </div>

      {drawerOpen ? (
        <>
          <div className="mobile-drawer-backdrop" onClick={() => setDrawerOpen(false)} />
          <aside className="mobile-drawer">
            <SidebarBrand onSelectWorkspace={() => setDrawerOpen(false)} />
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
