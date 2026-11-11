import { Kbd } from '@/components/ui/kbd'
import { shortcutText } from '@/shortcuts/shortcutText'
import { useBindings } from '@/shortcuts/useCommand'
import { createContext, Fragment, useContext, useEffect, useState, type ComponentType, type ReactNode } from 'react'
import { Link, useLocation, useMatch, useNavigate, useResolvedPath, type LinkProps } from 'react-router'
import { cva, type VariantProps } from 'class-variance-authority'
import { Calendar, ChevronDown, CalendarTick, Danger, SecurityUser, Home2 as Home, Layer, Message as MessageSquare, DocumentText as FileText, Search4 as Search, Setting2 as Settings, Sms as Mail, Star, TaskSquare as SquareCheck, Timer, Folder, Add as Plus, Trash } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { Tip } from '@/components/common/Tip'
import { ColorDot } from '@/components/common/ColorDot'
import { useQuery } from '@tanstack/react-query'
import { useProjectOrder, useProjects, useSetProjectOrder } from '@/features/tasks/api/projects'
import { moveProject, orderProjects } from '@orbit/api/projectOrder'
import type { ProjectRecord } from '@orbit/api/generated/types.gen'
import { ProjectLock } from '@/features/tasks/components/ProjectLock'
import { NewProjectModal } from '@/features/tasks/components/NewProjectModal'
import { savedViewsQueryOptions } from '@/features/views/api/views'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { useIsGuest } from '@/features/workspaces/permissions'
import { phonePath, settingsAreaOf, useSettingsArea, type SettingsAreaName } from '@/features/settings/settingsNav'
import { useIsPhone } from '@/features/chat/pages/useChatViewport'
import { chatEnabled, docsHidden } from './productNavigation'
import { hostsPageList } from './sidebarSlot'

const WORKSPACE_LINKS = [
  { to: '/', label: 'Home', icon: Home, enabled: true },
  { to: '/tasks', label: 'Tasks', icon: SquareCheck, enabled: true },
  { to: '/docs', label: 'Docs', icon: FileText, enabled: true },
  { to: '/chat', label: 'Chat', icon: MessageSquare, enabled: false },
  // slice 1 receives and reads mail; sending comes later
  { to: '/mail', label: 'Mail', icon: Mail, enabled: true, tag: 'Beta' },
].filter((link) => !(docsHidden && link.to === '/docs'))

// Coolify tile navigation: the row never fills; the icon sits in a 3D tile and the current row shows a pink tile.
// The Flat style (Account settings → Appearance) drops the tile: plain rows, and a filled row for the current page.
const sidebarNavItemClass =
  'group/nav-item relative flex h-9 w-full min-w-0 shrink-0 items-center justify-start gap-2.5 overflow-hidden pr-2.5 pl-1 text-left text-[0.8125rem] font-medium whitespace-nowrap text-sidebar-nav transition-colors duration-120 outline-none hover:text-sidebar-nav-active focus-visible:text-sidebar-nav-active data-[active=true]:text-sidebar-nav-active group-data-[collapsed=true]/sidebar-nav:justify-center group-data-[collapsed=true]/sidebar-nav:px-0 '
    + 'flat:h-8 flat:rounded-md flat:pl-2.5 flat:hover:not-disabled:bg-black/4 flat:data-[active=true]:bg-black/5 dark:flat:hover:not-disabled:bg-white/5 dark:flat:data-[active=true]:bg-white/6'

const sidebarNavTileVariants = cva(
  // 30px: a 16px icon, 6px padding and a 1px border, resting on a 2px ledge
  'box-content flex size-4 shrink-0 items-center justify-center rounded-lg border border-black/9 bg-neutral-50 p-1.5 shadow-[0_2px_0_var(--tile-depth)] transition-[translate,box-shadow,border-color,background-color] duration-80 [--tile-depth:rgb(0_0_0/0.22)] group-hover/nav-item:-translate-y-px group-hover/nav-item:border-black/18 group-hover/nav-item:bg-white group-hover/nav-item:shadow-[0_3px_0_var(--tile-depth)] group-active/nav-item:translate-y-0.5 group-active/nav-item:shadow-none group-disabled/nav-item:translate-y-0 group-disabled/nav-item:border-black/9 group-disabled/nav-item:bg-neutral-50 group-disabled/nav-item:shadow-[0_2px_0_var(--tile-depth)] dark:[--tile-depth:rgb(255_255_255/0.05)] [&_svg]:size-4 '
    // current: a pink-edged white tile in light mode, the pink gradient tile in dark mode (no glow: the row clips it)
    + 'group-data-[active=true]/nav-item:border-primary/45 group-data-[active=true]/nav-item:bg-white group-data-[active=true]/nav-item:text-primary group-data-[active=true]/nav-item:[--tile-depth:color-mix(in_oklab,var(--primary)_45%,transparent)] dark:group-data-[active=true]/nav-item:border-[color-mix(in_oklab,var(--primary),white_25%)] dark:group-data-[active=true]/nav-item:bg-primary dark:group-data-[active=true]/nav-item:bg-linear-to-b dark:group-data-[active=true]/nav-item:from-[color-mix(in_oklab,var(--primary),white_6%)] dark:group-data-[active=true]/nav-item:to-[color-mix(in_oklab,var(--primary),black_18%)] dark:group-data-[active=true]/nav-item:text-white dark:group-data-[active=true]/nav-item:[--tile-depth:color-mix(in_oklab,var(--primary),black_40%)] '
    // Flat: only the icon, in the row's colour (important: the tile's state classes are more specific)
    + 'flat:translate-y-0! flat:border-0! flat:bg-transparent! flat:bg-none! flat:p-0! flat:text-current! flat:shadow-none! flat:size-[18px] flat:[&_svg]:size-[18px]! flat:[&_svg]:opacity-90',
  {
    variants: {
      tone: {
        brand: 'dark:group-not-data-[active=true]/nav-item:border-sidebar-tint/13 dark:group-not-data-[active=true]/nav-item:bg-primary/7 dark:group-not-data-[active=true]/nav-item:text-sidebar-tint-foreground dark:group-not-data-[active=true]/nav-item:group-hover/nav-item:border-white/14 dark:group-not-data-[active=true]/nav-item:group-hover/nav-item:bg-white/6 dark:group-not-data-[active=true]/nav-item:group-hover/nav-item:text-white dark:group-disabled/nav-item:border-sidebar-tint/13! dark:group-disabled/nav-item:bg-primary/7! dark:group-disabled/nav-item:text-sidebar-tint-foreground!',
        // Admin is not a workspace app: a neutral tile unless it is the current page
        neutral: 'dark:group-not-data-[active=true]/nav-item:border-white/7 dark:group-not-data-[active=true]/nav-item:bg-white/3 dark:group-not-data-[active=true]/nav-item:text-[oklch(0.72_0_0)] dark:group-not-data-[active=true]/nav-item:group-hover/nav-item:border-white/14 dark:group-not-data-[active=true]/nav-item:group-hover/nav-item:bg-white/6 dark:group-not-data-[active=true]/nav-item:group-hover/nav-item:text-white',
      },
    },
    defaultVariants: { tone: 'brand' },
  },
)

/** The 3D icon tile: it lifts on hover, sinks on press, and turns pink on the current row. */
function SidebarNavTile({ icon: Icon, tone }: { icon: SidebarNavIcon } & VariantProps<typeof sidebarNavTileVariants>) {
  return (
    <span data-slot="sidebar-nav-tile" className={sidebarNavTileVariants({ tone })}>
      <Icon />
    </span>
  )
}

const SidebarNavCollapsed = createContext(false)

type SidebarNavIcon = ComponentType<{ className?: string }>

function SidebarNavLabel({ children }: { children: ReactNode }) {
  return <span data-slot="sidebar-nav-label" className="min-w-0 flex-1 truncate group-data-[collapsed=true]/sidebar-nav:hidden">{children}</span>
}

/** A sidebar link. Active when the route matches `to` (like NavLink), unless `active` says otherwise. */
function SidebarNavItem({
  to,
  end = false,
  icon: Icon,
  label,
  'aria-label': ariaLabel = label,
  active,
  badge = 0,
  unread = false,
  tag,
  tone,
  className,
  trailing,
  ...props
}: Omit<LinkProps, 'className' | 'children' | 'title'> & VariantProps<typeof sidebarNavTileVariants> & {
  className?: string
  end?: boolean
  icon: SidebarNavIcon
  label: string
  active?: boolean
  /** A count of things that wait for the user. Nothing shows for 0. */
  badge?: number
  /** Something waits for the user, but there is no count: a dot shows. */
  unread?: boolean
  /** A short note after the label, e.g. "Beta". Hidden while collapsed. */
  tag?: string
  /** A mark right after the label, hidden with the label when the sidebar is collapsed. */
  trailing?: ReactNode
}) {
  const collapsed = useContext(SidebarNavCollapsed)
  const match = useMatch({ path: useResolvedPath(to).pathname, end })
  const isActive = active ?? match !== null
  const name = badge > 0 ? `${ariaLabel}, ${badge} unread` : unread ? `${ariaLabel}, unread` : ariaLabel
  return (
    <Tip label={collapsed ? name : undefined} side="right">
      <Link
        data-slot="sidebar-nav-item"
        data-active={isActive}
        aria-current={isActive ? 'page' : undefined}
        aria-label={name}
        className={cn(sidebarNavItemClass, className)}
        to={to}
        {...props}
      >
        <SidebarNavTile icon={Icon} tone={tone} />
        <SidebarNavLabel>{label}</SidebarNavLabel>
        {tag ? (
          <span data-slot="sidebar-nav-tag" aria-hidden="true" className="shrink-0 text-[0.625rem] text-muted-foreground/70 group-data-[collapsed=true]/sidebar-nav:hidden">
            {tag}
          </span>
        ) : null}
        {trailing ? <span aria-hidden="true" className="flex shrink-0 group-data-[collapsed=true]/sidebar-nav:hidden">{trailing}</span> : null}
        {badge > 0 ? (
          <span
            data-slot="sidebar-nav-badge"
            aria-hidden="true"
            className="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-primary px-1 text-[0.625rem] leading-none font-semibold text-primary-foreground tabular-nums group-data-[collapsed=true]/sidebar-nav:absolute group-data-[collapsed=true]/sidebar-nav:top-0.5 group-data-[collapsed=true]/sidebar-nav:right-0.5"
          >
            {badge > 99 ? '99+' : badge}
          </span>
        ) : unread ? (
          <span
            data-slot="sidebar-nav-dot"
            aria-hidden="true"
            className="mr-1 size-2 shrink-0 rounded-full bg-primary group-data-[collapsed=true]/sidebar-nav:absolute group-data-[collapsed=true]/sidebar-nav:top-1 group-data-[collapsed=true]/sidebar-nav:right-1 group-data-[collapsed=true]/sidebar-nav:mr-0"
          />
        ) : null}
      </Link>
    </Tip>
  )
}

/** A product area that is not built yet: shown, but disabled. */
function SidebarNavComingSoon({ icon: Icon, label }: { icon: SidebarNavIcon; label: string }) {
  const collapsed = useContext(SidebarNavCollapsed)
  return (
    <Tip label={collapsed ? `${label} — Coming soon` : undefined} side="right">
      <Button
        data-slot="sidebar-nav-item"
        variant="ghost"
        className={cn(sidebarNavItemClass, 'rounded-none border-0 hover:bg-transparent hover:text-sidebar-nav disabled:pointer-events-auto disabled:cursor-not-allowed disabled:opacity-[0.48] dark:hover:bg-transparent')}
        disabled
      >
        <SidebarNavTile icon={Icon} />
        <SidebarNavLabel>{label}</SidebarNavLabel>
        <span className="ml-auto text-[0.625rem] text-muted-foreground/70 group-data-[collapsed=true]/sidebar-nav:hidden">Coming soon</span>
      </Button>
    </Tip>
  )
}

function SidebarSection({ label, collapsed }: { label: string; collapsed: boolean }) {
  if (collapsed) return <Separator className="mx-2.5 my-2 bg-black/8 data-horizontal:w-auto dark:bg-sidebar-tint/10" aria-hidden="true" />
  return (
    // Coolify's section header: Title Case in light, small mono capitals in dark.
    <div className="mt-3 px-2.5 py-1 text-[0.6875rem] leading-4 font-medium text-sidebar-nav-muted select-none dark:font-mono dark:text-[0.65625rem] dark:tracking-[0.08em] dark:uppercase">
      {label}
    </div>
  )
}

const PAGE_LIST_TITLES: Record<string, string> = { docs: 'Docs', chat: 'Chat' }

// The pages opened in this tab, newest last, so Back can leave the current area. Shared by the sidebar and the drawer.
const recentPaths: string[] = []
// Set by Back, so the next area slides in from the left even when Back lands in another area (Settings → Tasks).
let backPressed = false

/** Tasks, its saved views and its trash. */
const isTasksPath = (pathname: string) => /^\/(tasks|tasks-trash|tasks-import|views)(\/|$)/.test(pathname)

/** The area whose links or list replace the app links: a settings area, Tasks, or a page with its own list. */
function areaOf(pathname: string): string | null {
  return settingsAreaOf(pathname) ?? (isTasksPath(pathname) ? 'tasks' : hostsPageList(pathname) ? pathname.split('/')[1] : null)
}

type SidebarArea = SettingsAreaName | 'tasks'

/** The task pages ("presets") of the URL's `view`, as the old Personal section listed them. */
const TASK_PRESETS = [
  { view: 'mine', label: 'My tasks', icon: SquareCheck },
  { view: 'current_week', label: 'This week', icon: Calendar },
  { view: 'my_week', label: 'My week', icon: CalendarTick },
  { view: 'overdue', label: 'Overdue', icon: Danger },
  { view: 'due_soon', label: 'Due soon', icon: Timer },
] as const

// One icon component per colour, so a project's tile keeps its identity between renders.
const dotIcons = new Map<string, SidebarNavIcon>()
function dotIcon(color: string): SidebarNavIcon {
  let icon = dotIcons.get(color)
  if (!icon) {
    icon = ({ className }) => <ColorDot color={color} className={cn('size-2.5 rounded-[3px]', className)} />
    dotIcons.set(color, icon)
  }
  return icon
}

/**
 * The Tasks area: Views, the task pages, the projects and favourite views, as sidebar links.
 */
function TasksAreaNav({ onNavigate, onBack }: { onNavigate?: () => void; onBack?: () => void }) {
  const collapsed = useContext(SidebarNavCollapsed)
  const location = useLocation()
  const params = new URLSearchParams(location.search)
  const view = params.get('view')
  const project = params.get('project')
  const onTasks = location.pathname === '/tasks'
  const navigate = useNavigate()
  const [creating, setCreating] = useState(false)
  const workspaceId = useContext(WorkspaceContext)?.workspace.id ?? ''
  const projects = orderProjects(useProjects(workspaceId, workspaceId !== '').data ?? [], useProjectOrder(workspaceId, workspaceId !== '').data ?? [])
  const favorites = (useQuery({ ...savedViewsQueryOptions(workspaceId), enabled: workspaceId !== '' }).data ?? [])
    .filter((saved) => saved.is_favorite)
    .sort((a, b) => (a.favorite_position ?? 0) - (b.favorite_position ?? 0))

  const tasksPath = (next: { view?: string | null; project?: string | null }) => {
    const query = new URLSearchParams()
    const nextProject = next.project === undefined ? project : next.project
    const nextView = next.view === undefined ? view : next.view
    if (nextProject) query.set('project', nextProject)
    if (nextView) query.set('view', nextView)
    return `/tasks${query.size > 0 ? `?${query}` : ''}`
  }

  return (
    <>
      <BackToApp label="Tasks" onNavigate={onNavigate} onBack={onBack} />
      <SidebarNavItem to={tasksPath({ view: null })} active={onTasks && !view} icon={SquareCheck} label="All tasks" onClick={onNavigate} />
      <SidebarNavItem to="/views" end icon={Layer} label="Views" aria-label="Task views" onClick={onNavigate} />
      <SidebarSection label="My work" collapsed={collapsed} />
      {TASK_PRESETS.map((preset) => (
        <SidebarNavItem key={preset.view} to={tasksPath({ view: preset.view })} active={onTasks && view === preset.view} icon={preset.icon} label={preset.label} onClick={onNavigate} />
      ))}
      {projects.length > 0 ? (
        <>
          <SidebarSection label="Projects" collapsed={collapsed} />
          <SidebarNavItem to={tasksPath({ project: null })} active={onTasks && !project} icon={Folder} label="All projects" onClick={onNavigate} />
          <ProjectLinks workspaceId={workspaceId} projects={projects} selected={onTasks ? project : null} pathOf={(id) => tasksPath({ project: id })} onNavigate={onNavigate} />
        </>
      ) : <SidebarSection label="Projects" collapsed={collapsed} />}
      <Tip label={collapsed ? 'New project' : undefined} side="right">
        <button type="button" data-slot="sidebar-nav-item" className={sidebarNavItemClass} aria-label="New project" onClick={() => setCreating(true)}>
          <SidebarNavTile icon={Plus} />
          <SidebarNavLabel>New project</SidebarNavLabel>
        </button>
      </Tip>
      {creating ? (
        <NewProjectModal
          onClose={() => setCreating(false)}
          onCreated={(created) => {
            setCreating(false)
            onNavigate?.()
            navigate(tasksPath({ project: created.id }))
          }}
        />
      ) : null}
      {favorites.length > 0 ? (
        <>
          <SidebarSection label="Favorite views" collapsed={collapsed} />
          {favorites.map((saved) => (
            <SidebarNavItem key={saved.id} to={`/views/${saved.id}`} icon={Star} label={saved.name} onClick={onNavigate} />
          ))}
        </>
      ) : null}
      {/* at the bottom, like the Docs trash */}
      <div className="flex-1" aria-hidden="true" />
      <SidebarNavItem to="/tasks-trash" icon={Trash} label="Trash" aria-label="Task trash" onClick={onNavigate} />
    </>
  )
}

/**
 * The project links in the member's own order. A row drags to a new place, or moves with Alt+Arrow when its link has
 * focus; the order saves for this member only.
 */
function ProjectLinks({ workspaceId, projects, selected, pathOf, onNavigate }: {
  workspaceId: string
  projects: ProjectRecord[]
  selected: string | null
  pathOf: (projectId: string) => string
  onNavigate?: () => void
}) {
  const collapsed = useContext(SidebarNavCollapsed)
  const setOrder = useSetProjectOrder(workspaceId)
  const [dragId, setDragId] = useState<string | null>(null)
  const [overId, setOverId] = useState<string | null>(null)
  const ids = projects.map((item) => item.id)
  const move = (id: string, to: number) => {
    const next = moveProject(ids, id, to)
    if (next) setOrder.mutate(next)
  }
  const endDrag = () => {
    setDragId(null)
    setOverId(null)
  }

  return projects.map((item, index) => {
    const target = overId === item.id && dragId !== null && dragId !== item.id
    return (
      <div
        key={item.id}
        // the dragged row fades and stays mounted (a drag cancels if its source leaves the DOM); a 2px pink line marks the drop
        className="group/project relative data-[dragging=true]:opacity-50 data-[drop-target=true]:before:absolute data-[drop-target=true]:before:inset-x-2 data-[drop-target=true]:before:z-10 data-[drop-target=true]:before:h-0.5 data-[drop-target=true]:before:rounded-full data-[drop-target=true]:before:bg-primary data-[drop-edge=before]:before:-top-px data-[drop-edge=after]:before:-bottom-px"
        draggable
        data-dragging={dragId === item.id || undefined}
        data-drop-target={target || undefined}
        // dropping takes the target's slot, so a downward move lands below it and an upward one above
        data-drop-edge={target && dragId !== null ? (ids.indexOf(dragId) < index ? 'after' : 'before') : undefined}
        onDragStart={(event) => {
          setDragId(item.id)
          event.dataTransfer.effectAllowed = 'move'
          event.dataTransfer.setData('text/plain', item.id)
        }}
        onDragOver={(event) => {
          if (!dragId) return
          event.preventDefault()
          event.dataTransfer.dropEffect = 'move'
          setOverId(item.id)
        }}
        onDrop={(event) => {
          event.preventDefault()
          if (dragId) move(dragId, index)
          endDrag()
        }}
        onDragEnd={endDrag}
      >
        <SidebarNavItem
          to={pathOf(item.id)}
          active={selected === item.id}
          icon={dotIcon(item.color)}
          label={item.name}
          aria-label={item.private ? `${item.name}, private` : item.name}
          trailing={item.private ? <ProjectLock project={item} className="text-sidebar-nav" /> : undefined}
          className="pr-8"
          // the row is the drag source, not the link (a dragged link carries its URL and a text drag image)
          draggable={false}
          aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
          onClick={onNavigate}
          onKeyDown={(event) => {
            if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return
            event.preventDefault()
            move(item.id, event.key === 'ArrowUp' ? index - 1 : index + 1)
            // React moves the focused node while reordering; keep focus on it for the next press
            const link = event.currentTarget
            requestAnimationFrame(() => link.focus())
          }}
        />
        {collapsed ? null : (
          <Tip label="Project settings" side="right">
            <Link
              to={`/tasks/projects/${item.id}/settings`}
              aria-label={`${item.name} settings`}
              draggable={false}
              className="absolute top-1/2 right-1 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-sidebar-nav opacity-0 transition-opacity outline-none group-hover/project:opacity-100 hover:bg-black/5 hover:text-sidebar-nav-active focus-visible:opacity-100 pointer-coarse:opacity-100 dark:hover:bg-white/8"
              onClick={onNavigate}
            >
              <Settings className="size-3.5" />
            </Link>
          </Tip>
        )}
      </div>
    )
  })
}

/**
 * Coolify's Back row: a small chevron as wide as an icon tile, then the area's name, centred. No tile, a fill on hover.
 * It opens the last page outside the current area.
 */
function BackToApp({ label, onNavigate, onBack }: { label: string; onNavigate?: () => void; onBack?: () => void }) {
  const collapsed = useContext(SidebarNavCollapsed)
  const area = areaOf(useLocation().pathname)
  const index = recentPaths.findLastIndex((path) => areaOf(path.split('?')[0]) !== area)
  const target = recentPaths[index] ?? '/'
  // Back pops: the pages after the target are forgotten, so the next Back goes further back instead of returning here
  const goBack = () => {
    backPressed = true
    recentPaths.length = Math.max(index, 0)
    onNavigate?.()
  }
  const props = {
    'data-slot': 'sidebar-nav-back',
    title: 'Back to the main menu',
    'aria-label': 'Back to the main menu',
    className: 'mb-1 flex h-9 w-full min-w-0 shrink-0 items-center gap-2.5 rounded-md pr-2.5 pl-1 text-[0.8125rem] outline-none transition-colors duration-120 hover:bg-neutral-100 focus-visible:outline-1 focus-visible:outline-primary dark:hover:bg-white/6 group-data-[collapsed=true]/sidebar-nav:justify-center group-data-[collapsed=true]/sidebar-nav:px-0 flat:h-8 flat:pl-2.5',
    children: (
      <>
        {/* as wide as an icon tile (16px icon, padding and border), so the chevron lines up with the icons below */}
        <span className="flex h-4 w-[30px] shrink-0 items-center justify-center flat:w-[18px]">
          <ChevronDown className="size-3 rotate-90 text-primary dark:text-sidebar-nav" />
        </span>
        <span className="min-w-0 flex-1 truncate pr-6 text-center font-medium text-foreground group-data-[collapsed=true]/sidebar-nav:hidden">{label}</span>
      </>
    ),
  }
  return (
    <Tip label={collapsed ? 'Back to the main menu' : undefined} side="right">
      {/* with `onBack` (the mobile drawer) Back only shows the main menu, like Coolify's drawer: the page stays */}
      {onBack ? <button type="button" {...props} onClick={onBack} /> : <Link {...props} to={target} onClick={goBack} />}
    </Tip>
  )
}

/** Coolify's content-aware sidebar: inside Settings, Account settings or Admin, the area's links replace the app links. */
function SettingsAreaNav({ area, onNavigate, onBack }: { area: SettingsAreaName; onNavigate?: () => void; onBack?: () => void }) {
  const { title } = useSettingsArea(area)
  return (
    <>
      <BackToApp label={title} onNavigate={onNavigate} onBack={onBack} />
      <SettingsAreaLinks area={area} onNavigate={onNavigate} />
    </>
  )
}

/** A settings area's sections and links. `NoWorkspaceShell` shows them without Back: there is no app to go back to. */
export function SettingsAreaLinks({ area, profileOnly, onNavigate }: { area: SettingsAreaName; profileOnly?: boolean; onNavigate?: () => void }) {
  const collapsed = useContext(SidebarNavCollapsed)
  const phone = useIsPhone()
  return useSettingsArea(area, profileOnly).sections.map((section) => (
    <Fragment key={section.label}>
      <SidebarSection label={section.label} collapsed={collapsed} />
      {section.items.map((item) => (
        <SidebarNavItem key={item.to} to={phone ? phonePath(item) : item.to} end={item.end} icon={item.icon} label={item.label} onClick={onNavigate} />
      ))}
    </Fragment>
  ))
}

/** Grouped sidebar navigation — shared by the desktop sidebar and the mobile drawer. */
export function SidebarNav({
  onNavigate,
  collapsed = false,
  chatBadge,
  mailBadge,
  mailEnabled = false,
  chatUnread,
  chatEnabled: chat = chatEnabled,
  isRoot = false,
  backShowsMenu = false,
  pageListRef,
}: {
  onNavigate?: () => void
  collapsed?: boolean
  /** Unread count on the Chat item. */
  chatBadge?: number
  /** Unread Inbox threads on the Mail item. */
  mailBadge?: number
  /** Mail is on for the server; off, Mail shows as Coming soon. */
  mailEnabled?: boolean
  /** Chat has an unread message, with or without a count. */
  chatUnread?: boolean
  /** Chat is a link instead of "Coming soon". Defaults to the build's setting. */
  chatEnabled?: boolean
  /** The root user (installation administrator) also gets the Admin link. */
  isRoot?: boolean
  /** The mobile drawer: Back shows the main menu without leaving the page. Closing the drawer forgets it. */
  backShowsMenu?: boolean
  /** Set while the open page's own list (docs, chat) shows here instead of the app links: the element it renders in. */
  pageListRef?: (element: HTMLDivElement | null) => void
}) {
  const location = useLocation()
  // inside Tasks, Settings, Account settings or Admin the area's links and Back replace the app links;
  // a guest has Chat and its account settings only (`GuestWorkspaceGate`)
  const guest = useIsGuest()
  const pageArea: SidebarArea | null = settingsAreaOf(location.pathname) ?? (isTasksPath(location.pathname) && !guest ? 'tasks' : null)
  // the area whose Back was pressed in the drawer: the main menu shows until the page moves to another area
  const [mainMenuFor, setMainMenuFor] = useState<SidebarArea | null>(null)
  const area = pageArea && pageArea === mainMenuFor ? null : pageArea
  const pageList = pageListRef !== undefined && hostsPageList(location.pathname)
  // What the sidebar shows below Search. Only a change animates, so the first load and moves inside an area stay still.
  const content = pageList ? `list:${areaOf(location.pathname)}` : (area ?? 'app')
  const [shown, setShown] = useState(content)
  const [enter, setEnter] = useState<'forward' | 'back' | null>(null)
  if (shown !== content) {
    setShown(content)
    setEnter(content === 'app' || backPressed ? 'back' : 'forward')
  }
  useEffect(() => {
    backPressed = false
  }, [content])
  useEffect(() => {
    const path = location.pathname + location.search
    if (recentPaths.at(-1) !== path) recentPaths.push(path)
    if (recentPaths.length > 50) recentPaths.shift()
  }, [location.pathname, location.search])
  const currentParams = new URLSearchParams(location.search)
  const view = currentParams.get('view')
  const selectedProject = currentParams.get('project')

  const tasksPath = selectedProject ? `/tasks?${new URLSearchParams({ project: selectedProject })}` : '/tasks'

  const paletteKeys = useBindings()['palette.open']

  const openSearch = () => {
    onNavigate?.()
    window.dispatchEvent(new CustomEvent('open-command-palette'))
  }

  // Tasks itself is active on its routes only while no task view is picked
  const tasksActive = useMatch({ path: '/tasks', end: false }) !== null && !view

  return (
    <SidebarNavCollapsed.Provider value={collapsed}>
      <div data-slot="sidebar-nav" data-collapsed={collapsed} className="group/sidebar-nav flex min-h-0 flex-1 flex-col">
        {guest ? null : (
        <div className="px-1 pb-3">
        <Tip label={collapsed ? 'Search' : undefined} side="right">
          {collapsed ? (
            <button type="button" data-slot="sidebar-nav-item" className={sidebarNavItemClass} aria-label="Search" onClick={openSearch}>
              <SidebarNavTile icon={Search} />
            </button>
          ) : (
            // the expanded sidebar shows Search as an input, not a tile
            <button
              type="button"
              className="flex h-8 w-full min-w-0 items-center gap-2.5 rounded-md bg-neutral-100 pr-1.5 pl-2.5 text-[0.8125rem] font-medium text-muted-foreground transition-colors outline-none hover:bg-neutral-200 focus-visible:outline-1 focus-visible:outline-primary dark:bg-sidebar-tint/6 dark:inset-ring dark:inset-ring-sidebar-tint/10 dark:hover:bg-sidebar-tint/10"
              aria-label="Search"
              onClick={openSearch}
            >
              <Search className="size-4 shrink-0 opacity-90" />
              <span className="min-w-0 flex-1 truncate text-left">Search</span>
              {paletteKeys ? (
                <Kbd className="h-auto rounded-md border border-neutral-300 bg-neutral-200 px-1.5 py-0.5 text-[0.6875rem] font-medium text-muted-foreground dark:border-sidebar-tint/18 dark:bg-white/6">{shortcutText(paletteKeys)}</Kbd>
              ) : null}
            </button>
          )}
        </Tip>
        </div>
        )}
        {/* a new area slides in: forward from the right, back to the app links from the left */}
        <div
          key={content}
          data-slot="sidebar-nav-content"
          data-enter={enter ?? undefined}
          className="flex min-h-0 flex-1 flex-col data-enter:animate-in data-enter:fade-in data-enter:duration-200 data-[enter=back]:slide-in-from-left-6 data-[enter=forward]:slide-in-from-right-6 motion-reduce:animate-none"
        >
          {pageList ? (
            <>
              {/* a guest has Chat alone: there is no main menu to go back to */}
              {guest ? null : <BackToApp label={PAGE_LIST_TITLES[areaOf(location.pathname) ?? ''] ?? ''} onNavigate={onNavigate} />}
              {/* the page renders its list here (`InAppSidebar`); it scrolls on its own */}
              <div ref={pageListRef} data-slot="sidebar-page-list" className="-mx-3 mt-2 flex min-h-0 flex-1 flex-col" />
            </>
          ) : (
            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto overscroll-contain px-1 pb-2">
              {area === 'tasks' ? <TasksAreaNav onNavigate={onNavigate} onBack={backShowsMenu ? () => setMainMenuFor(area) : undefined} /> : area ? <SettingsAreaNav area={area} onNavigate={onNavigate} onBack={backShowsMenu ? () => setMainMenuFor(area) : undefined} /> : guest ? (
                <>
                  <SidebarNavItem to="/chat" icon={MessageSquare} label="Chat" badge={chatBadge} unread={chatUnread} onClick={onNavigate} />
                  {isRoot ? (
                    <>
                      <div className="flex-1" aria-hidden="true" />
                      <SidebarNavItem to="/admin" icon={SecurityUser} tone="neutral" label="Admin" onClick={onNavigate} />
                    </>
                  ) : null}
                </>
              ) : (
                <>
                  {WORKSPACE_LINKS.map((link) => (link.to === '/mail' ? mailEnabled : link.enabled || (link.to === '/chat' && chat)) ? (
                    <SidebarNavItem
                      key={link.to}
                      to={link.to === '/tasks' ? tasksPath : link.to}
                      active={link.to === '/tasks' ? tasksActive : undefined}
                      end={link.to === '/'}
                      icon={link.icon}
                      label={link.label}
                      tag={link.tag}
                      badge={link.to === '/chat' ? chatBadge : link.to === '/mail' ? mailBadge : undefined}
                      unread={link.to === '/chat' ? chatUnread : undefined}
                      onClick={onNavigate}
                    />
                  ) : (
                    <SidebarNavComingSoon key={link.to} icon={link.icon} label={link.label} />
                  ))}
                  <SidebarSection label="Manage" collapsed={collapsed} />
                  <SidebarNavItem to="/settings" icon={Settings} label="Settings" onClick={onNavigate} />
                  {isRoot ? (
                    <>
                      {/* the installation, not the workspace: it sits apart at the bottom */}
                      <div className="flex-1" aria-hidden="true" />
                      <SidebarNavItem to="/admin" icon={SecurityUser} tone="neutral" label="Admin" onClick={onNavigate} />
                    </>
                  ) : null}
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </SidebarNavCollapsed.Provider>
  )
}
