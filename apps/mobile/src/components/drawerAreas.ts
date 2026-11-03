/** The settings areas (the web's `settingsAreaOf`). */
export type SettingsArea = 'workspace' | 'account' | 'admin'
/** The areas whose links replace the app links in the new UI's drawer: a settings area or Tasks. */
export type DrawerArea = SettingsArea | 'tasks'

export const DRAWER_AREA_TITLES: Record<DrawerArea, string> = { workspace: 'Settings', account: 'Account settings', admin: 'Admin', tasks: 'Tasks' }

interface AreaLink {
  href: string
}

const under = (pathname: string, root: string) => pathname === root || pathname.startsWith(`${root}/`)

/**
 * The area a route belongs to: Tasks for the task routes, else the settings area of the link whose route is the
 * longest prefix of it, so `/settings/admin/jobs` is Admin and `/settings/profile` is Account settings, while any other
 * page under `/settings` is the workspace's.
 */
export function drawerAreaOf(pathname: string, areas: Record<SettingsArea, readonly AreaLink[]>): DrawerArea | null {
  if (under(pathname, '/tasks')) return 'tasks'
  let found: SettingsArea | null = null
  let length = -1
  for (const [area, links] of Object.entries(areas) as [SettingsArea, readonly AreaLink[]][]) {
    for (const link of links) {
      if (link.href.length > length && under(pathname, link.href)) {
        found = area
        length = link.href.length
      }
    }
  }
  return found
}

/** The web's section labels (`settingsNav.ts`); a link not listed here sits in its area's first section. */
const SECTIONS: Record<SettingsArea, { label: string; hrefs?: readonly string[] }[]> = {
  workspace: [{ label: 'Workspace' }, { label: 'Integrations', hrefs: ['/settings/file-storage'] }],
  account: [
    { label: 'Account' },
    { label: 'Preferences', hrefs: ['/settings/appearance', '/settings/notifications'] },
    { label: 'Advanced', hrefs: ['/settings/agentic'] },
  ],
  admin: [
    { label: 'Instance' },
    { label: 'Data', hrefs: ['/settings/admin/storage', '/settings/admin/backups'] },
    { label: 'Monitoring', hrefs: ['/settings/admin/audit', '/settings/admin/jobs'] },
  ],
}

/** An area's links grouped under the web's section labels, in the web's section order; empty sections are left out. */
export function drawerSections<T extends AreaLink>(area: SettingsArea, links: readonly T[]): { label: string; items: T[] }[] {
  const sections = SECTIONS[area]
  const groups = sections.map((section) => ({ label: section.label, items: [] as T[] }))
  for (const link of links) {
    const index = sections.findIndex((section) => section.hrefs?.includes(link.href))
    groups[Math.max(index, 0)].items.push(link)
  }
  return groups.filter((group) => group.items.length > 0)
}

/** The task presets of the URL's `view` (the web's `TASK_PRESETS`). */
export const TASK_PRESETS = [
  { view: 'mine', label: 'My tasks', icon: 'tasks' },
  { view: 'current_week', label: 'This week', icon: 'calendar' },
  { view: 'my_week', label: 'My week', icon: 'calendarTick' },
  { view: 'overdue', label: 'Overdue', icon: 'danger' },
  { view: 'due_soon', label: 'Due soon', icon: 'timer' },
] as const

type TaskQuery = { view?: string | null; project?: string | null }

/** The tasks route for `next`: a key left out keeps the current value, `null` clears it. */
export function tasksHref(current: TaskQuery, next: TaskQuery): `/tasks${string}` {
  const query = new URLSearchParams()
  const project = next.project === undefined ? current.project : next.project
  const view = next.view === undefined ? current.view : next.view
  if (project) query.set('project', project)
  if (view) query.set('view', view)
  const search = query.toString()
  return `/tasks${search ? `?${search}` : ''}`
}
