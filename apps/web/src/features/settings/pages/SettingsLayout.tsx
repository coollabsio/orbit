import { Navigate, Outlet, useLocation } from 'react-router'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useIsInstallationAdmin } from '@/features/workspaces/permissions'
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
 * Coolify `x-settings.layout`: the content column. The desktop app sidebar and the mobile drawer show this area's
 * links (`SidebarNav`), so the page has no sub-navigation of its own.
 */
function SettingsShell({ area }: { area: SettingsAreaName }) {
  const { title, icon: Icon, sections } = useSettingsArea(area)
  const { pathname } = useLocation()
  // the link NavLink marks as current; the longest match wins, so `/settings/members` beats `/settings`
  const page = sections
    .flatMap((section) => section.items)
    .filter((item) => pathname === item.to || (!item.end && pathname.startsWith(`${item.to}/`)))
    .sort((a, b) => b.to.length - a.to.length)[0]
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane className="relative">
        {/* same left inset as the content column, so the title lines up with the cards */}
        <PaneHeader className="gap-[7px] bg-background pl-5 text-muted-foreground min-[900px]:pl-8 xl:pl-10">
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
          <section className="grid w-full min-w-0 grid-cols-1 gap-6 px-5 pt-5 pb-8 min-[900px]:px-8 min-[900px]:pt-6 min-[900px]:pb-10 xl:gap-8 xl:px-10 xl:pt-7">
            <div className="flex min-w-0 flex-col gap-6">
              <Outlet />
            </div>
          </section>
        </div>
      </Pane>
    </div>
  )
}
