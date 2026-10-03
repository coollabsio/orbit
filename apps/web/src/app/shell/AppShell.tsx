import { ShortcutProvider } from '@/shortcuts/ShortcutProvider'
import { useShortcutBindings } from '@/shortcuts/useShortcutBindings'
import { SequenceHint } from '@/shortcuts/SequenceHint'
import { ShortcutHelpDialog } from '@/shortcuts/ShortcutHelpDialog'
import { NavigationCommands, useGlobalCommands } from './globalCommands'
import { NewTaskProvider } from '@/features/tasks/newTask'
import { useCommand } from '@/shortcuts/useCommand'
import { useEffect, useLayoutEffect, useState } from 'react'
import { useWorkspaceEvents } from '@/features/realtime/useWorkspaceEvents'
import { ChatProvider } from '@/features/chat/api/ChatProvider'
import { useChatBadgeCount } from '@/features/chat/api/queries'
import { useHasUnreadNotifications } from '@/features/inbox/api'
import { startPushWorker } from '@/features/realtime/push'
import { useUnreadFavicon } from '@/lib/favicon'
import { chatEnabled } from './productNavigation'
import { Outlet, useLocation } from 'react-router'
import { SidebarLeft as PanelLeft } from 'reicon-react'
import { cn } from 'cn'
import { SideSheet, SideSheetContent } from '@/components/common/SideSheet'
import { Button } from '@/components/ui/button'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'
import { CommandPalette } from './CommandPalette'
import { SidebarNav } from './SidebarNav'
import { Topbar } from './Topbar'
import { UserMenu } from './UserMenu'
import { MobileDock } from './MobileDock'
import { ProfilePopoverProvider } from './ProfilePopover'

/** The signed-in app. Shortcuts exist only here, with the user's own keys; the sign-in pages have none. */
export function AppShell() {
  const { overrides } = useShortcutBindings()
  return (
    <ShortcutProvider overrides={overrides}>
      <ChatProvider enabled={chatEnabled}>
        <Shell />
      </ChatProvider>
    </ShortcutProvider>
  )
}

function Shell() {
  const { workspace } = useWorkspace()
  const live = useWorkspaceEvents(workspace.id)
  const chatBadge = useChatBadgeCount()
  // The tab icon gets a dot while chat or the Inbox has something unread.
  const inboxUnread = useHasUnreadNotifications(workspace.id).data ? 1 : 0
  useUnreadFavicon(chatBadge + inboxUnread)
  // Signed in: the worker that shows notifications may run. This asks for no permission.
  useEffect(() => startPushWorker(), [])
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [collapsedPreference, setCollapsedPreference] = useState(() => window.localStorage.getItem('orbit:sidebar_collapsed') === 'true')
  // Chat has its own sidebar, so the app sidebar collapses there. Expanding it in chat lasts for the visit and does not
  // change the saved preference of the other pages.
  const { pathname } = useLocation()
  const inChat = pathname === '/chat' || pathname.startsWith('/chat/')
  const [chatExpanded, setChatExpanded] = useState(false)
  const sidebarCollapsed = inChat ? !chatExpanded : collapsedPreference
  const toggleSidebar = () => (inChat ? setChatExpanded((expanded) => !expanded) : setCollapsedPreference((collapsed) => !collapsed))

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

  useCommand('palette.open', () => setPaletteOpen((open) => !open))
  const [helpOpen, setHelpOpen] = useState(false)
  useGlobalCommands({ openPalette: () => setPaletteOpen(true), openHelp: () => setHelpOpen(true) })

  useEffect(() => {
    const onOpen = () => setPaletteOpen(true)
    const onOpenSidebar = () => setDrawerOpen(true)
    window.addEventListener('open-command-palette', onOpen)
    window.addEventListener('open-sidebar', onOpenSidebar)
    return () => {
      window.removeEventListener('open-command-palette', onOpen)
      window.removeEventListener('open-sidebar', onOpenSidebar)
    }
  }, [])

  useEffect(() => {
    window.localStorage.setItem('orbit:sidebar_collapsed', String(collapsedPreference))
  }, [collapsedPreference])

  return (
    <ProfilePopoverProvider>
    <NewTaskProvider>
    <div className="flex h-[var(--app-height,100svh)] w-full overflow-hidden bg-background pt-[env(safe-area-inset-top,0px)] pl-[env(safe-area-inset-left,0px)] pr-[env(safe-area-inset-right,0px)]">
      {!live ? (
        <div
          className="pointer-events-none fixed right-3 bottom-16 z-50 rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-muted-foreground duration-200 animate-in fade-in fill-mode-both delay-1000"
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
        <SidebarNav collapsed={sidebarCollapsed} chatBadge={chatBadge} />
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
            onClick={toggleSidebar}
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
        <MobileDock chatBadge={chatBadge} />
      </div>

      <SideSheet open={drawerOpen} onOpenChange={setDrawerOpen}>
        <SideSheetContent
          side="left"
          aria-label="Navigation"
          className="z-[41] w-[min(280px,84vw)] overflow-hidden border-r border-border bg-sidebar px-3 pt-[env(safe-area-inset-top,0px)] pb-[calc(12px+env(safe-area-inset-bottom,0px))] text-sidebar-foreground"
        >
          <div className="mb-2 flex h-12 shrink-0 items-center justify-start gap-2 px-1.5">
            <WorkspaceSwitcher onSelect={() => setDrawerOpen(false)} />
          </div>
          <SidebarNav onNavigate={() => setDrawerOpen(false)} chatBadge={chatBadge} />
          <div className="flex shrink-0 items-center gap-2 border-t border-border pt-2">
            <UserMenu />
          </div>
        </SideSheetContent>
      </SideSheet>

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
      {helpOpen ? <ShortcutHelpDialog onClose={() => setHelpOpen(false)} /> : null}
      <NavigationCommands />
      <SequenceHint />
    </div>
    </NewTaskProvider>
    </ProfilePopoverProvider>
  )
}
