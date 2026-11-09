import { useState } from 'react'
import { Link, Outlet, useLocation } from 'react-router'
import { Add, SidebarRight2 } from 'reicon-react'
import { cn } from 'cn'
import { SideSheet, SideSheetContent } from '@/components/common/SideSheet'
import { Button, buttonVariants } from '@/components/ui/button'
import { MenuSurfaceContext } from '@/components/ui/menuSurface'
import { useIsInstallationAdmin } from '@/features/workspaces/permissions'
import { useAppSidebarTheme } from '@/lib/customTheme'
import { useSidebarMenuSurface } from './sidebarMenu'
import { SettingsAreaLinks } from './SidebarNav'
import { UserMenu } from './UserMenu'

/**
 * The app of someone with no workspace, framed like the workspace app: a sidebar on desktop, a top bar and a drawer
 * on a phone. Operators (the root user or an instance admin) get Admin and their account settings; anyone else only
 * their profile, where they can delete the account. "Create workspace" opens the onboarding.
 */
export function NoWorkspaceShell() {
  const isOperator = useIsInstallationAdmin()
  const home = isOperator ? '/admin' : '/profile'
  const sidebarTheme = useAppSidebarTheme()
  const menuSurface = useSidebarMenuSurface()
  const [drawerOpen, setDrawerOpen] = useState(false)
  // any move to another page closes the drawer, as in the workspace app
  const { pathname } = useLocation()
  const [drawerPage, setDrawerPage] = useState(pathname)
  if (drawerPage !== pathname) {
    setDrawerPage(pathname)
    setDrawerOpen(false)
  }
  const links = (
    <nav aria-label="Settings" data-slot="sidebar-nav" className="group/sidebar-nav -mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto overscroll-contain px-1 pb-2">
      {isOperator ? <SettingsAreaLinks area="admin" /> : null}
      <SettingsAreaLinks area="profile" profileOnly={!isOperator} />
    </nav>
  )
  const createWorkspace = (
    <div className="px-1 pb-3">
      <Link to="/onboarding" className={cn(buttonVariants({ variant: 'outline' }), 'w-full gap-1.5')}>
        <Add className="size-4" />
        Create workspace
      </Link>
    </div>
  )
  return (
    <div className="flex h-[var(--app-height,100svh)] w-full overflow-hidden bg-background pt-[env(safe-area-inset-top,0px)] pl-[env(safe-area-inset-left,0px)] pr-[env(safe-area-inset-right,0px)] min-[900px]:floating:gap-2 min-[900px]:floating:p-2 min-[900px]:floating:bg-[radial-gradient(circle,rgb(0_0_0/0.13)_1px,transparent_1.3px)] min-[900px]:floating:bg-size-[20px_20px] dark:min-[900px]:floating:bg-[radial-gradient(circle,rgb(255_255_255/0.09)_1px,transparent_1.3px)]">
      <aside
        className={cn(sidebarTheme.scheme, 'relative flex w-56 shrink-0 flex-col overflow-hidden border-r border-sidebar-edge bg-sidebar bg-linear-to-b from-sidebar-wash to-transparent to-[520px] px-3 text-sidebar-foreground max-[899px]:hidden floating:rounded-lg floating:border')}
        style={sidebarTheme.style}
      >
        <MenuSurfaceContext value={menuSurface}>
          {/* the workspace switcher's look, with Orbit in place of a workspace name */}
          <div className="mb-2 flex h-12 shrink-0 items-center px-1.5">
            <Link to={home} className="truncate text-[17px] font-bold tracking-[-0.02em] text-foreground">Orbit</Link>
          </div>
          {createWorkspace}
          {links}
          <div className="flex shrink-0 items-center py-2">
            <UserMenu />
          </div>
        </MenuSurfaceContext>
      </aside>
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-background min-[900px]:floating:overflow-hidden min-[900px]:floating:rounded-lg min-[900px]:floating:border">
        {/* the workspace app's phone top bar (`Topbar`) */}
        <header className="hidden shrink-0 items-center justify-between gap-4 border-b border-border bg-card px-4 py-3 max-[899px]:flex">
          <Link to={home} className="flex min-w-0 flex-1 items-center gap-2.5">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted">
              <img src="/logo.svg" alt="" className="size-[18px]" />
            </span>
            <span className="truncate text-[13px] font-semibold text-foreground">Orbit</span>
          </Link>
          <div className="flex shrink-0 items-center gap-1">
            <UserMenu topbar />
            <Button variant="ghost" size="icon-lg" className="-mr-1 text-muted-foreground" aria-label="Open sidebar" onClick={() => setDrawerOpen(true)}>
              <SidebarRight2 className="size-5" />
            </Button>
          </div>
        </header>
        <div className="flex min-h-0 min-w-0 flex-1">
          <Outlet />
        </div>
      </div>
      <SideSheet open={drawerOpen} onOpenChange={setDrawerOpen}>
        <SideSheetContent
          side="right"
          aria-label="Navigation"
          className={cn(sidebarTheme.scheme, 'z-[41] flex w-[min(280px,84vw)] flex-col overflow-hidden rounded-l-2xl border-l border-sidebar-edge bg-sidebar bg-linear-to-b from-sidebar-wash to-transparent to-[520px] px-3 pt-[calc(12px+env(safe-area-inset-top,0px))] pb-[calc(12px+env(safe-area-inset-bottom,0px))] text-sidebar-foreground')}
          style={sidebarTheme.style}
        >
          <MenuSurfaceContext value={menuSurface}>
            {createWorkspace}
            {links}
          </MenuSurfaceContext>
        </SideSheetContent>
      </SideSheet>
    </div>
  )
}
