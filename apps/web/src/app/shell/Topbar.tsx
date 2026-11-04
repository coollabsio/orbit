import { Link, useLocation } from 'react-router'
import { SidebarRight2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { ActivityBell } from './ActivityBell'
import { isChatConversationPath } from './productNavigation'
import { UserMenu } from './UserMenu'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

/**
 * The phone's top bar, as in Coolify: logo, workspace switcher, Activity, account menu and the button that opens the navigation
 * drawer. Pages keep their own headers below it. A chat conversation is a full screen, so the bar is not shown there.
 */
export function Topbar({ onOpenDrawer, inboxUnread = false }: { onOpenDrawer: () => void; /** Activity has an unread notification. */ inboxUnread?: boolean }) {
  const { pathname } = useLocation()
  if (isChatConversationPath(pathname)) return null
  return (
    <header className="hidden shrink-0 items-center justify-between gap-4 border-b border-border bg-card px-4 py-3 max-[899px]:flex">
      <div className="flex min-w-0 flex-1 items-center gap-2.5">
        <Link to="/" aria-label="Home" className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted transition-opacity hover:opacity-80">
          <img src="/logo.svg" alt="" className="size-[18px]" />
        </Link>
        <WorkspaceSwitcher topbar />
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <ActivityBell topbar unread={inboxUnread} />
        <UserMenu topbar />
        <Button variant="ghost" size="icon-lg" className="-mr-1 text-muted-foreground" aria-label="Open sidebar" onClick={onOpenDrawer}>
          <SidebarRight2 className="size-5" />
        </Button>
      </div>
    </header>
  )
}
