import { useContext } from 'react'
import { Link, Navigate, Outlet, useLocation } from 'react-router'
import { ArrowLeft, ChevronRight } from 'reicon-react'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useIsPhone } from '@/features/chat/pages/useChatViewport'
import { useIsInstallationAdmin } from '@/features/workspaces/permissions'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { phonePath, useSettingsArea, type SettingsAreaName, type SettingsNavSection } from '../settingsNav'

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
 * links (`SidebarNav`). On a phone the area's root is a menu of its pages, and each page has Back to that menu.
 */
function SettingsShell({ area }: { area: SettingsAreaName }) {
  // someone with no workspace who is not an operator may open only their profile
  const workspace = useContext(WorkspaceContext)
  const operator = useIsInstallationAdmin()
  const profileOnly = area === 'profile' && !workspace && !operator
  const { title, icon: Icon, sections } = useSettingsArea(area, profileOnly)
  const { pathname } = useLocation()
  const items = sections.flatMap((section) => section.items)
  const root = items[0]?.to
  const hasMenu = useIsPhone() && items.length > 1
  const showMenu = hasMenu && pathname.replace(/\/$/, '') === root
  // the link NavLink marks as current; the longest match wins, so `/settings/members` beats `/settings`
  const page = showMenu
    ? undefined
    : items
        .filter((item) => pathname === item.to || pathname === phonePath(item) || (!item.end && pathname.startsWith(`${item.to}/`)))
        .sort((a, b) => b.to.length - a.to.length)[0]
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane className="relative">
        {/* same left inset as the content column, so the title lines up with the cards */}
        <PaneHeader className={`gap-[7px] bg-background text-muted-foreground min-[900px]:pl-8 xl:pl-10 ${hasMenu && !showMenu ? 'pl-2' : 'pl-5'}`}>
          {hasMenu && !showMenu && root ? (
            <Link to={root} aria-label={`Back to ${title}`} className="-mr-0.5 flex size-8 shrink-0 items-center justify-center rounded-md hover:bg-muted hover:text-foreground">
              <ArrowLeft className="size-[18px]" />
            </Link>
          ) : null}
          <Icon className="size-[15px] shrink-0" />
          <span className="truncate text-[0.8125rem] font-semibold">{title}</span>
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
              {showMenu ? <SettingsMenu sections={sections} /> : <Outlet />}
            </div>
          </section>
        </div>
      </Pane>
    </div>
  )
}

/** A phone's settings menu: the area's sections as cards of links, like the native app's lists. */
function SettingsMenu({ sections }: { sections: SettingsNavSection[] }) {
  return sections.map((section) => (
    <nav key={section.label} aria-label={section.label} className="flex flex-col gap-2">
      <h2 className="px-1 text-xs font-medium text-muted-foreground">{section.label}</h2>
      <ul className="overflow-hidden rounded-lg bg-card shadow-[0_0_0_1px_var(--border)]">
        {section.items.map((item) => (
          <li key={item.to} className="not-first:border-t">
            <Link to={phonePath(item)} className="flex min-h-12 items-center gap-3 px-4 text-sm font-medium text-foreground active:bg-muted">
              <item.icon className="size-[18px] shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate">{item.label}</span>
              <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  ))
}
