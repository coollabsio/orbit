import type { ComponentType } from 'react'
import { useRender } from '@base-ui/react/use-render'
import { NavLink, useLocation } from 'react-router'
import { Home2 as Home, Message as MessageSquare, DocumentText as FileText, Setting2 as Settings, Sms as Mail, TaskSquare as SquareCheck } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { useIsGuest } from '@/features/workspaces/permissions'
import { chatEnabled, docsHidden, isChatConversationPath, mobileDockPathsFor } from './productNavigation'

const DOCK_LINKS = [
  { to: '/', label: 'Home', icon: Home },
  { to: '/tasks', label: 'Tasks', icon: SquareCheck },
  { to: '/docs', label: 'Docs', icon: FileText },
  { to: '/chat', label: 'Chat', icon: MessageSquare },
  { to: '/mail', label: 'Mail', icon: Mail },
  { to: '/settings', label: 'Settings', icon: Settings },
].filter((link) => !(docsHidden && link.to === '/docs'))

/** A guest's dock: Chat and its own account settings. */
const GUEST_DOCK_LINKS = [
  { to: '/chat', label: 'Chat', icon: MessageSquare },
  { to: '/profile', label: 'Settings', icon: Settings },
]

/** A dock entry: a link for the sections that exist, a disabled button for the upcoming ones. */
function MobileDockItem({ to, label, icon: Icon, enabled, badge = 0, unread = false }: { to: string; label: string; icon: ComponentType<{ className?: string }>; enabled: boolean; badge?: number; /** A dot when there is no count. */ unread?: boolean }) {
  const count = enabled ? badge : 0
  const dot = enabled && count === 0 && unread
  return useRender({
    render: enabled ? <NavLink to={to} end={to === '/'} aria-label={count > 0 ? `${label}, ${count} unread` : dot ? `${label}, unread` : undefined} /> : <Button variant="ghost" disabled aria-label={label} />,
    props: {
      'data-slot': 'mobile-dock-item',
      className: cn(
        'relative flex flex-1 flex-col items-center justify-center gap-0.5 text-[0.625rem] font-medium text-muted-foreground',
        enabled
          ? 'aria-[current=page]:text-primary'
          : 'h-auto rounded-none p-0 hover:bg-transparent dark:hover:bg-transparent disabled:pointer-events-auto disabled:cursor-not-allowed disabled:text-muted-foreground/50 disabled:opacity-100',
      ),
      children: (
        <>
          <span className="relative">
            <Icon className="size-5" />
            {count > 0 ? (
              <span
                data-slot="mobile-dock-badge"
                aria-hidden="true"
                className="absolute -top-1 left-3 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[0.625rem] leading-none font-semibold text-primary-foreground tabular-nums"
              >
                {count > 99 ? '99+' : count}
              </span>
            ) : dot ? (
              <span data-slot="mobile-dock-dot" aria-hidden="true" className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-primary" />
            ) : null}
          </span>
          {label}
        </>
      ),
    },
  })
}

/**
 * The phone's bottom navigation. A chat conversation is a full screen, so the dock is not shown there.
 * It also hides while the keyboard is open, so the field being typed in keeps the space.
 */
export function MobileDock({ chatEnabled: chat = chatEnabled, chatBadge = 0, chatUnread = false, mailBadge = 0 }: { chatEnabled?: boolean; /** Unread count on the Chat item. */ chatBadge?: number; /** Chat has an unread message, with or without a count. */ chatUnread?: boolean; /** Unread Inbox threads on the Mail item. */ mailBadge?: number }) {
  const { pathname } = useLocation()
  const guest = useIsGuest()
  if (isChatConversationPath(pathname)) return null
  const links = guest ? GUEST_DOCK_LINKS : DOCK_LINKS
  const paths = guest ? GUEST_DOCK_LINKS.map((link) => link.to) : mobileDockPathsFor(chat)
  return (
    <nav
      className="hidden h-auto min-h-14 shrink-0 items-stretch bg-background pt-3 [@media(display-mode:standalone)]:pb-[env(safe-area-inset-bottom,0px)] max-[899px]:not-in-data-keyboard:flex"
      aria-label="Mobile navigation"
    >
      {links.map((link) => (
        <MobileDockItem key={link.to} {...link} enabled={paths.includes(link.to)} badge={link.to === '/chat' ? chatBadge : link.to === '/mail' ? mailBadge : 0} unread={link.to === '/chat' && chatUnread} />
      ))}
    </nav>
  )
}
