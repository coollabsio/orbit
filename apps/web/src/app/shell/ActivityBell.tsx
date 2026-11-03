import { Link, useLocation } from 'react-router'
import { Bell } from 'reicon-react'
import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'

/** Activity, beside the workspace name: it stays in view in every sidebar area, and a dot shows while something is unread. */
export function ActivityBell({ unread = false, topbar = false }: { unread?: boolean; /** The phone top bar's look: page colours instead of the sidebar's. */ topbar?: boolean }) {
  const { pathname, search } = useLocation()
  // keep the project picked in Tasks, so Activity opens filtered to it
  const project = new URLSearchParams(search).get('project')
  const active = pathname === '/activity' || pathname.startsWith('/activity/')
  const name = unread ? 'Activity, unread' : 'Activity'
  return (
    <Tip label="Activity" side="bottom">
      <Link
        data-slot="activity-bell"
        data-active={active}
        aria-current={active ? 'page' : undefined}
        aria-label={name}
        to={project ? `/activity?project=${encodeURIComponent(project)}` : '/activity'}
        className={cn(
          'relative flex shrink-0 items-center justify-center rounded-md transition-colors outline-none focus-visible:outline-1 focus-visible:outline-primary',
          topbar
            ? 'size-9 rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-muted data-[active=true]:text-foreground'
            : 'size-8 text-sidebar-nav hover:bg-black/4 hover:text-sidebar-nav-active data-[active=true]:bg-black/4 data-[active=true]:text-sidebar-nav-active dark:hover:bg-white/5 dark:data-[active=true]:bg-white/5',
        )}
      >
        <Bell className={topbar ? 'size-5' : 'size-[18px] opacity-90'} />
        {unread ? <span data-slot="activity-bell-dot" aria-hidden="true" className={cn('absolute size-2 rounded-full bg-primary ring-2', topbar ? 'top-2 right-2 ring-card' : 'top-1.5 right-1.5 ring-sidebar')} /> : null}
      </Link>
    </Tip>
  )
}
