import { NavLink } from 'react-router'
import { Home, MessageSquare, MessagesSquare, FileText, Settings, Mail, SquareCheck } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { mobileDockPaths } from './productNavigation'

const DOCK_LINKS = [
  { to: '/', label: 'Home', icon: Home },
  { to: '/tasks', label: 'Tasks', icon: SquareCheck },
  { to: '/docs', label: 'Docs', icon: FileText },
  { to: '/mail', label: 'Mail', icon: Mail },
  { to: '/chat', label: 'Chat', icon: MessageSquare },
  { to: '/dm', label: 'DMs', icon: MessagesSquare },
  { to: '/settings', label: 'Settings', icon: Settings },
]

const itemClass =
  'relative flex flex-1 flex-col items-center justify-center gap-0.5 text-[10px] font-medium text-muted-foreground'

export function MobileDock() {
  return (
    <nav
      className="hidden h-auto min-h-14 shrink-0 items-stretch bg-background pt-3 [@media(display-mode:standalone)]:pb-[env(safe-area-inset-bottom,0px)] max-[899px]:flex"
      aria-label="Mobile navigation"
    >
      {DOCK_LINKS.map((link) => mobileDockPaths.some((path) => path === link.to) ? (
        <NavLink key={link.to} to={link.to} className="flex flex-1 items-stretch">
          {({ isActive }) => (
            <span className={cn(itemClass, isActive && 'text-primary')} data-active={isActive}>
              <link.icon className="size-5" />
              {link.label}
            </span>
          )}
        </NavLink>
      ) : (
        <Button
          key={link.to}
          variant="ghost"
          className={cn(itemClass, 'h-auto rounded-none p-0 hover:bg-transparent dark:hover:bg-transparent disabled:pointer-events-auto disabled:cursor-not-allowed disabled:text-muted-foreground/50 disabled:opacity-100')}
          disabled
          aria-label={link.label}
        >
          <link.icon className="size-5" />
          {link.label}
        </Button>
      ))}
    </nav>
  )
}
