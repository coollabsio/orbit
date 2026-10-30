import { useContext } from 'react'
import { Navigate, NavLink, Outlet, useLocation } from 'react-router'
import { cn } from 'cn'
import { buttonVariants } from '@/components/ui/button'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { useIsInstallationAdmin } from '@/features/workspaces/permissions'
import { useNewUi } from '@/lib/themeContext'
import { useSettingsArea, type SettingsAreaName } from '../settingsNav'

/** The workspace's settings. */
export function SettingsLayout() {
  return <SettingsShell area="settings" />
}

/** The signed-in person's own settings. */
export function AccountLayout() {
  return <SettingsShell area="profile" />
}

/** The root user's Admin area. The server refuses every admin request from other accounts; this only hides the pages. */
export function AdminLayout() {
  if (!useIsInstallationAdmin()) return <Navigate to="/tasks" replace />
  return <SettingsShell area="admin" />
}

/**
 * Coolify `x-settings.layout`: sticky sub-navigation (210px) + content column. In the new UI the desktop app sidebar
 * and the mobile drawer show this area's links (`SidebarNav`), so there the page has no sub-navigation of its own.
 */
function SettingsShell({ area }: { area: SettingsAreaName }) {
  const { title, icon: Icon, sections } = useSettingsArea(area)
  const newUi = useNewUi()
  const { pathname } = useLocation()
  // the link NavLink marks as current; the longest match wins, so `/settings/members` beats `/settings`
  const page = sections
    .flatMap((section) => section.items)
    .filter((item) => pathname === item.to || (!item.end && pathname.startsWith(`${item.to}/`)))
    .sort((a, b) => b.to.length - a.to.length)[0]
  // the new UI's app sidebar shows these links (`SidebarNav`); the classic one does not
  const inAppShell = useContext(WorkspaceContext) !== null && newUi
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane className="relative">
        {/* hidden on mobile so the shell topbar takes over; same left inset as the content column, so the title lines up with the cards */}
        <PaneHeader className="gap-[7px] bg-background pl-8 text-muted-foreground max-[899px]:hidden xl:pl-10">
          <Icon className="size-[15px] shrink-0" />
          <span className="truncate text-[13px] font-semibold">{title}</span>
          {page ? (
            <>
              <span aria-hidden>/</span>
              <PaneTitle render={<h1 />}>{page.label}</PaneTitle>
            </>
          ) : null}
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <section className={cn('grid w-full min-w-0 grid-cols-1 gap-6 px-5 pt-5 pb-8 min-[900px]:px-8 min-[900px]:pt-6 min-[900px]:pb-10 xl:gap-8 xl:px-10 xl:pt-7', !inAppShell && 'xl:grid-cols-[210px_minmax(0,1fr)]')}>
            {inAppShell ? null : (
              <aside className={cn('min-w-0 xl:sticky xl:top-7 xl:max-h-[calc(100dvh-5.5rem)] xl:self-start xl:overflow-x-hidden xl:overflow-y-auto xl:pr-1.5 xl:[overscroll-behavior:contain]')}>
                <nav
                  aria-label={title}
                  className="grid grid-cols-2 gap-0.5 border-y py-3 min-[900px]:grid-cols-4 xl:grid-cols-1 xl:border-0 xl:py-0"
                >
                  {sections.map((section, index) => (
                    <div key={section.label} className="contents">
                      <div
                        className={cn(
                          'col-span-full px-2.5 py-1 text-[11px] font-medium text-sidebar-foreground/60 select-none max-xl:hidden',
                          index > 0 && 'xl:mt-5 xl:border-t xl:pt-4',
                        )}
                      >
                        {section.label}
                      </div>
                      {section.items.map((item) => (
                        <NavLink
                          key={item.to}
                          to={item.to}
                          end={item.end}
                          // NavLink marks the current page with aria-current
                          className={cn(buttonVariants({ variant: 'ghost' }), 'w-full min-w-0 justify-start gap-2.5 text-[13px] aria-[current=page]:bg-sidebar-accent aria-[current=page]:text-sidebar-accent-foreground')}
                        >
                          <item.icon className="size-4.5 opacity-90" />
                          <span className="min-w-0 flex-1 truncate">{item.label}</span>
                        </NavLink>
                      ))}
                    </div>
                  ))}
                </nav>
              </aside>
            )}
            <div className="flex min-w-0 flex-col gap-6">
              <Outlet />
            </div>
          </section>
        </div>
      </Pane>
    </div>
  )
}
