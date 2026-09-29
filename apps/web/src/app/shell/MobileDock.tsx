import type { ComponentType } from 'react'
import { useRender } from '@base-ui/react/use-render'
import { NavLink } from 'react-router'
import { Home2 as Home, Message as MessageSquare, Messages2 as MessagesSquare, DocumentText as FileText, Setting2 as Settings, Sms as Mail, TaskSquare as SquareCheck } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { docsHidden, mobileDockPaths } from './productNavigation'

const DOCK_LINKS = [
  { to: '/', label: 'Home', icon: Home },
  { to: '/tasks', label: 'Tasks', icon: SquareCheck },
  { to: '/docs', label: 'Docs', icon: FileText },
  { to: '/mail', label: 'Mail', icon: Mail },
  { to: '/chat', label: 'Chat', icon: MessageSquare },
  { to: '/dm', label: 'DMs', icon: MessagesSquare },
  { to: '/settings', label: 'Settings', icon: Settings },
].filter((link) => !(docsHidden && link.to === '/docs'))

/** A dock entry: a link for the sections that exist, a disabled button for the upcoming ones. */
function MobileDockItem({ to, label, icon: Icon, enabled }: { to: string; label: string; icon: ComponentType<{ className?: string }>; enabled: boolean }) {
  return useRender({
    render: enabled ? <NavLink to={to} /> : <Button variant="ghost" disabled aria-label={label} />,
    props: {
      'data-slot': 'mobile-dock-item',
      className: cn(
        'relative flex flex-1 flex-col items-center justify-center gap-0.5 text-[10px] font-medium text-muted-foreground',
        enabled
          ? 'aria-[current=page]:text-primary'
          : 'h-auto rounded-none p-0 hover:bg-transparent dark:hover:bg-transparent disabled:pointer-events-auto disabled:cursor-not-allowed disabled:text-muted-foreground/50 disabled:opacity-100',
      ),
      children: (
        <>
          <Icon className="size-5" />
          {label}
        </>
      ),
    },
  })
}

export function MobileDock() {
  return (
    <nav
      className="hidden h-auto min-h-14 shrink-0 items-stretch bg-background pt-3 [@media(display-mode:standalone)]:pb-[env(safe-area-inset-bottom,0px)] max-[899px]:flex"
      aria-label="Mobile navigation"
    >
      {DOCK_LINKS.map((link) => (
        <MobileDockItem key={link.to} {...link} enabled={mobileDockPaths.some((path) => path === link.to)} />
      ))}
    </nav>
  )
}
