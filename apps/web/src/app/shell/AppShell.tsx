import { useEffect, useLayoutEffect, useState } from 'react'
import { useWorkspaceEvents } from '@/features/realtime/useWorkspaceEvents'
import { Outlet } from 'react-router'
import { PanelLeft } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'
import { CommandPalette } from './CommandPalette'
import { SidebarNav } from './SidebarNav'
import { Topbar } from './Topbar'
import { UserMenu } from './UserMenu'
import { MobileDock } from './MobileDock'

export function AppShell() {
  const { workspace } = useWorkspace()
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

  return (
    <div className="flex h-[var(--app-height,100svh)] w-full overflow-hidden bg-background pt-[env(safe-area-inset-top,0px)] pl-[env(safe-area-inset-left,0px)] pr-[env(safe-area-inset-right,0px)]">
      {!live ? (
        <div
          className="pointer-events-none fixed right-3 bottom-16 z-50 rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-muted-foreground"
          role="status"
        >
          Connecting to live updates…
        </div>
      ) : null}
      <aside
        className={cn(
          'flex shrink-0 flex-col overflow-hidden border-r border-border bg-sidebar px-2 pb-2 text-sidebar-foreground transition-[width,padding] duration-200 ease-out max-[899px]:hidden',
          sidebarCollapsed ? 'w-14' : 'w-56',
        )}
        data-collapsed={sidebarCollapsed || undefined}
      >
        <div
          className={cn(
            'mb-2 flex h-12 shrink-0 items-center gap-2',
            sidebarCollapsed ? 'justify-center px-0' : 'justify-between px-1.5',
          )}
        >
          <WorkspaceSwitcher collapsed={sidebarCollapsed} />
        </div>
        <SidebarNav collapsed={sidebarCollapsed} />
        <div
          className={cn(
            'flex shrink-0 items-center gap-2 border-t border-border pt-2',
            sidebarCollapsed && 'flex-col justify-center gap-1.5',
          )}
        >
          <UserMenu collapsed={sidebarCollapsed} />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="shrink-0 text-muted-foreground/70"
            aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
          >
            <PanelLeft className={cn('size-[17px]', sidebarCollapsed && 'rotate-180')} />
          </Button>
        </div>
      </aside>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
        <Topbar onOpenDrawer={() => setDrawerOpen(true)} onOpenPalette={() => setPaletteOpen(true)} />
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <Outlet />
        </div>
        <MobileDock />
      </div>

      {drawerOpen ? (
        <>
          <div className="fixed inset-0 z-[80] bg-black/50" onClick={() => setDrawerOpen(false)} />
          <aside className="fixed top-0 bottom-0 left-0 z-[81] flex w-[min(280px,84vw)] flex-col overflow-hidden border-r border-border bg-sidebar px-3 pt-[env(safe-area-inset-top,0px)] pb-[calc(12px+env(safe-area-inset-bottom,0px))] text-sidebar-foreground duration-200 animate-in fade-in slide-in-from-left-6">
            <div className="mb-2 flex h-12 shrink-0 items-center justify-start gap-2 px-1.5">
              <WorkspaceSwitcher onSelect={() => setDrawerOpen(false)} />
            </div>
            <SidebarNav onNavigate={() => setDrawerOpen(false)} />
            <div className="flex shrink-0 items-center gap-2 border-t border-border pt-2">
              <UserMenu />
            </div>
          </aside>
        </>
      ) : null}

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
    </div>
  )
}
