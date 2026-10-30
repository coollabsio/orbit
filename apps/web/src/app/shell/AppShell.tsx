import { ShortcutProvider } from '@/shortcuts/ShortcutProvider'
import { useShortcutBindings } from '@/shortcuts/useShortcutBindings'
import { SequenceHint } from '@/shortcuts/SequenceHint'
import { ShortcutHelpDialog } from '@/shortcuts/ShortcutHelpDialog'
import { NavigationCommands, useGlobalCommands } from './globalCommands'
import { NewTaskProvider } from '@/features/tasks/newTask'
import { useCommand } from '@/shortcuts/useCommand'
import { useContext, useEffect, useLayoutEffect, useState, useSyncExternalStore } from 'react'
import { useWorkspaceEvents } from '@/features/realtime/useWorkspaceEvents'
import { ChatProvider } from '@/features/chat/api/ChatProvider'
import { useChatBadges } from '@/features/chat/api/queries'
import { useHasUnreadNotifications } from '@/features/inbox/api'
import { startPushWorker } from '@/features/realtime/push'
import { preloadPageEditorWhenIdle } from '@/features/docs/editor/loadPageEditor'
import { useUnreadFavicon } from '@/lib/favicon'
import { chatEnabled } from './productNavigation'
import { Outlet, useLocation } from 'react-router'
import { SidebarLeft as PanelLeft } from 'reicon-react'
import { cn } from 'cn'
import { SideSheet, SideSheetContent } from '@/components/common/SideSheet'
import { Tip } from '@/components/common/Tip'
import { useWorkspace, WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { useIsInstallationAdmin } from '@/features/workspaces/permissions'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'
import { CommandPalette } from './CommandPalette'
import { ConnectionBanner } from './ConnectionBanner'
import { MailServerBanner } from './MailServerBanner'
import { ReadOnlyBanner } from './ReadOnlyBanner'
import { SidebarNav } from './SidebarNav'
import { Topbar } from './Topbar'
import { UserMenu } from './UserMenu'
import { SidebarResizeHandle, usePageListWidth } from './pageListWidth'
import { MobileDock } from './MobileDock'
import { ProfilePopoverProvider } from './ProfilePopover'
import { OperatorShell } from './OperatorShell'
import { useAppSidebarTheme } from '@/lib/customTheme'
import { MenuSurfaceContext } from '@/components/ui/menuSurface'
import { useNewUi } from '@/lib/themeContext'
import { useSidebarMenuSurface } from './sidebarMenu'
import { hostsPageList, SidebarSlotContext } from './sidebarSlot'

const desktopQuery = '(min-width: 900px)'
const subscribeDesktop = (onChange: () => void) => {
  const media = window.matchMedia(desktopQuery)
  media.addEventListener('change', onChange)
  return () => media.removeEventListener('change', onChange)
}
const isDesktop = () => window.matchMedia(desktopQuery).matches

/** The signed-in app. An operator with no workspace (`WorkspaceProvider` lets only them in) gets Admin alone. */
export function AppShell() {
  return useContext(WorkspaceContext) ? <WorkspaceShell /> : <OperatorShell />
}

/** Shortcuts exist only here, with the user's own keys; the sign-in pages have none. */
function WorkspaceShell() {
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
  const { total: chatBadge, unread: chatUnread } = useChatBadges()
  // The tab icon gets a dot while chat or the Inbox has something unread.
  const inboxUnread = useHasUnreadNotifications(workspace.id).data === true
  const isRoot = useIsInstallationAdmin()
  useUnreadFavicon(chatBadge + (inboxUnread ? 1 : 0))
  // Signed in: the worker that shows notifications may run. This asks for no permission.
  useEffect(() => startPushWorker(), [])
  // The rich text editor (tasks, comments, docs, the new task dialog) is ready before its first use.
  useEffect(() => preloadPageEditorWhenIdle(), [])
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  // One saved preference for every page, so collapsing the sidebar in chat keeps it collapsed in tasks too.
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => window.localStorage.getItem('orbit:sidebar_collapsed') === 'true')
  const toggleSidebar = () => setSidebarCollapsed((collapsed) => !collapsed)
  // Coolify's content-aware sidebar: the open sidebar holds the page's own list (docs, chat) instead of the app links.
  // Collapsed or on a phone, the page keeps its list beside the content.
  const desktop = useSyncExternalStore(subscribeDesktop, isDesktop)
  const { pathname, search } = useLocation()
  // Any move to another page closes the mobile drawer: sidebar links, the account menu (Appearance…), Back and the rest
  const [drawerPage, setDrawerPage] = useState(pathname + search)
  if (drawerPage !== pathname + search) {
    setDrawerPage(pathname + search)
    setDrawerOpen(false)
  }
  // only in the new UI (Account settings → Appearance → Layout)
  const newUi = useNewUi()
  const hostsList = newUi && desktop && !sidebarCollapsed && hostsPageList(pathname)
  // a long page tree needs room: the sidebar that holds it can be dragged wider
  const [pageListWidth, setPageListWidth] = usePageListWidth()
  const [resizing, setResizing] = useState(false)
  const [pageListSlot, setPageListSlot] = useState<HTMLElement | null>(null)
  // Account settings → Appearance → Custom colours the sidebar too, not only chat
  const sidebarTheme = useAppSidebarTheme()
  // the new UI's sidebar menus share one look and the custom theme; the classic ones keep their own
  const sidebarMenuSurface = useSidebarMenuSurface()
  const menuSurface = newUi ? sidebarMenuSurface : null

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
    window.localStorage.setItem('orbit:sidebar_collapsed', String(sidebarCollapsed))
  }, [sidebarCollapsed])

  return (
    <ProfilePopoverProvider>
    <NewTaskProvider>
    <SidebarSlotContext value={hostsList ? pageListSlot : null}>
    {/* Floating navigation (Account settings → Appearance, desktop): framed panels on a dotted backdrop */}
    <div className="flex h-[var(--app-height,100svh)] w-full overflow-hidden bg-background pt-[env(safe-area-inset-top,0px)] pl-[env(safe-area-inset-left,0px)] pr-[env(safe-area-inset-right,0px)] min-[900px]:floating:gap-2 min-[900px]:floating:p-2 min-[900px]:floating:bg-[radial-gradient(circle,rgb(0_0_0/0.13)_1px,transparent_1.3px)] min-[900px]:floating:bg-size-[20px_20px] dark:min-[900px]:floating:bg-[radial-gradient(circle,rgb(255_255_255/0.09)_1px,transparent_1.3px)]">
      <aside
        className={cn(
          sidebarTheme.scheme,
          'relative flex shrink-0 flex-col overflow-hidden border-r border-sidebar-edge bg-sidebar bg-linear-to-b from-sidebar-wash to-transparent to-[520px] px-3 text-sidebar-foreground transition-[width,padding] duration-200 ease-out max-[899px]:hidden floating:rounded-lg floating:border',
          sidebarCollapsed ? 'w-16' : !hostsList && 'w-56',
          resizing && 'transition-none select-none',
        )}
        data-collapsed={sidebarCollapsed || undefined}
        style={hostsList ? { ...sidebarTheme.style, width: pageListWidth } : sidebarTheme.style}
      >
        {hostsList && <SidebarResizeHandle width={pageListWidth} onResize={setPageListWidth} onDragging={setResizing} />}
        <MenuSurfaceContext value={menuSurface}>
          <div
            className={cn(
              'mb-2 flex h-12 shrink-0 items-center gap-2',
              sidebarCollapsed ? 'justify-center px-0' : 'justify-between px-1.5',
            )}
          >
            <WorkspaceSwitcher collapsed={sidebarCollapsed} />
          </div>
          <SidebarNav contextual={newUi} collapsed={sidebarCollapsed} chatBadge={chatBadge} chatUnread={chatUnread} inboxUnread={inboxUnread} isRoot={isRoot} pageListRef={hostsList ? setPageListSlot : undefined} />
          <div
            className={cn(
              'flex shrink-0 items-center justify-between gap-1 py-2',
              sidebarCollapsed && 'flex-col justify-center',
            )}
          >
            <UserMenu collapsed={sidebarCollapsed} />
            <Tip label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'} side={sidebarCollapsed ? 'right' : undefined}>
              <button
                type="button"
                className="flex size-8 shrink-0 items-center justify-center rounded-md text-sidebar-nav transition-colors outline-none hover:bg-black/4 hover:text-sidebar-nav-active focus-visible:outline-1 focus-visible:outline-primary dark:hover:bg-white/5"
                aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
                onClick={toggleSidebar}
              >
                <PanelLeft className={cn('size-[18px] opacity-90', sidebarCollapsed && 'rotate-180')} />
              </button>
            </Tip>
          </div>
        </MenuSurfaceContext>
      </aside>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background min-[900px]:floating:overflow-hidden min-[900px]:floating:rounded-lg min-[900px]:floating:border">
        <ConnectionBanner live={live} />
        <MailServerBanner />
        <ReadOnlyBanner />
        <Topbar onOpenDrawer={() => setDrawerOpen(true)} onOpenPalette={() => setPaletteOpen(true)} />
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <Outlet />
        </div>
        <MobileDock chatBadge={chatBadge} chatUnread={chatUnread} />
      </div>

      <SideSheet open={drawerOpen} onOpenChange={setDrawerOpen}>
        <SideSheetContent
          side="left"
          aria-label="Navigation"
          className={cn(sidebarTheme.scheme, 'z-[41] w-[min(280px,84vw)] overflow-hidden border-r border-sidebar-edge bg-sidebar bg-linear-to-b from-sidebar-wash to-transparent to-[520px] px-3 pt-[env(safe-area-inset-top,0px)] pb-[calc(12px+env(safe-area-inset-bottom,0px))] text-sidebar-foreground')}
          style={sidebarTheme.style}
        >
          <MenuSurfaceContext value={menuSurface}>
            <div className="mb-2 flex h-12 shrink-0 items-center justify-start gap-2 px-1.5">
              <WorkspaceSwitcher onSelect={() => setDrawerOpen(false)} />
            </div>
            <SidebarNav contextual={newUi} backShowsMenu onNavigate={() => setDrawerOpen(false)} chatBadge={chatBadge} chatUnread={chatUnread} inboxUnread={inboxUnread} isRoot={isRoot} />
            <div className="flex shrink-0 items-center pt-2">
              <UserMenu />
            </div>
          </MenuSurfaceContext>
        </SideSheetContent>
      </SideSheet>

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
      {helpOpen ? <ShortcutHelpDialog onClose={() => setHelpOpen(false)} /> : null}
      <NavigationCommands />
      <SequenceHint />
    </div>
    </SidebarSlotContext>
    </NewTaskProvider>
    </ProfilePopoverProvider>
  )
}
