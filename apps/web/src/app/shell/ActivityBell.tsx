import { Link, useLocation } from 'react-router'
import { Bell } from 'reicon-react'
import { Tip } from '@/components/common/Tip'

/** Activity, beside the workspace name: it stays in view in every sidebar area, and a dot shows while something is unread. */
export function ActivityBell({ unread = false }: { unread?: boolean }) {
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
        className="relative flex size-8 shrink-0 items-center justify-center rounded-md text-sidebar-nav transition-colors outline-none hover:bg-black/4 hover:text-sidebar-nav-active focus-visible:outline-1 focus-visible:outline-primary data-[active=true]:bg-black/4 data-[active=true]:text-sidebar-nav-active dark:hover:bg-white/5 dark:data-[active=true]:bg-white/5"
      >
        <Bell className="size-[18px] opacity-90" />
        {unread ? <span data-slot="activity-bell-dot" aria-hidden="true" className="absolute top-1.5 right-1.5 size-2 rounded-full bg-primary ring-2 ring-sidebar" /> : null}
      </Link>
    </Tip>
  )
}
