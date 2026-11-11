import { type Href, router, useGlobalSearchParams, usePathname } from 'expo-router'
import { createContext, Fragment, type ReactNode, use, useEffect, useMemo, useRef, useState } from 'react'
import { Animated, Easing, Modal, PanResponder, type PanResponderGestureState, Pressable, ScrollView, useWindowDimensions, View } from 'react-native'
import { NativeText as RNText } from '../ui/scaled'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import Svg, { Defs, LinearGradient, Rect, Stop } from 'react-native-svg'

import { useAuthOptions } from '../hooks/account'
import { useGithubSettings } from '../hooks/workspace-settings'
import { useOrbit } from '../orbit-context'
import { useChatBadge, useProjects } from '../queries'
import { splitAlpha } from '../data/color'
import { customSidebarTheme } from '../data/custom-theme'
import { alpha, cssColors, mix, useButtonStyle, useCustomTheme, useTheme } from '../theme'
import { ColorScope } from '../theme/ColorScope'
import { CountBadge, Icon, type IconName, useReduceMotion } from '../ui'
import { NewProjectSheet } from './NewProjectSheet'
import { DRAWER_AREA_TITLES, type DrawerArea, drawerAreaOf, drawerSections, TASK_PRESETS, tasksHref } from './drawerAreas'
import { ACCOUNT_NAV, ADMIN_NAV, allowedNav, WORKSPACE_NAV } from './settings/Shell'
import { guestSettings, isGuestRole } from '../data/guest'

const DrawerContext = createContext<(() => void) | null>(null)

/** Opens the drawer: the top bar's sidebar button. */
export function useOpenDrawer() {
  return use(DrawerContext) ?? undefined
}

/** The tab roots: a pushed screen keeps its own gestures. */
const SWIPE_ROOTS = new Set(['/', '/tasks', '/docs', '/chat', '/settings', '/activity'])
/** A swipe opens the drawer only when it starts this close to the right edge. */
const EDGE = 24

/** A clear horizontal move, so taps and vertical scrolling stay with the screen. */
const horizontal = (gesture: PanResponderGestureState, direction: 1 | -1) =>
  gesture.dx * direction > 10 && Math.abs(gesture.dx) > Math.abs(gesture.dy) * 1.5

const clamp = (value: number) => Math.min(1, Math.max(0, value))

/**
 * Holds the drawer of the tab screens, on the right like the web's phone sheet. Mount once, around the tabs. The top
 * bar's sidebar button opens it; a swipe from the right edge of a tab root drags it open and a swipe to the right
 * drags it closed, following the finger.
 */
export function DrawerProvider({ children }: { children: ReactNode }) {
  const { width: screen } = useWindowDimensions()
  const width = Math.min(280, screen * 0.84)
  const pathname = usePathname()
  const [mounted, setMounted] = useState(false)
  const progress = useRef(new Animated.Value(0)).current
  // The pan responders are created once: they read the current values through this ref.
  const live = useRef({ width, screen, mounted, swipeable: false })
  live.current = { width, screen, mounted, swipeable: SWIPE_ROOTS.has(pathname) }

  const settle = (open: boolean, velocity = 0) => {
    if (open) setMounted(true)
    const duration = Math.abs(velocity) > 1 ? 120 : open ? 200 : 150
    Animated.timing(progress, { toValue: open ? 1 : 0, duration, easing: Easing.out(Easing.cubic), useNativeDriver: true }).start(({ finished }) => {
      if (finished && !open) setMounted(false)
    })
  }

  // Any move to another page or view closes the drawer, like the web's mobile sheet.
  const route = `${pathname}?${JSON.stringify(useGlobalSearchParams())}`
  const shownRoute = useRef(route)
  const close = useRef(settle)
  close.current = settle
  useEffect(() => {
    if (shownRoute.current === route) return
    shownRoute.current = route
    if (live.current.mounted) close.current(false)
  }, [route])

  // Where the touch began: `gesture.x0` is not that on every platform (react-native-web), so record it.
  const touchStartX = useRef(Number.POSITIVE_INFINITY)
  const edge = useRef(
    PanResponder.create({
      onStartShouldSetPanResponderCapture: (event) => {
        touchStartX.current = event.nativeEvent.pageX
        return false
      },
      onMoveShouldSetPanResponderCapture: (_, gesture) => live.current.swipeable && !live.current.mounted && touchStartX.current >= live.current.screen - EDGE && horizontal(gesture, -1),
      onPanResponderGrant: () => {
        progress.setValue(0)
        setMounted(true)
      },
      onPanResponderMove: (_, gesture) => progress.setValue(clamp(-gesture.dx / live.current.width)),
      onPanResponderRelease: (_, gesture) => settle(-gesture.dx > live.current.width / 3 || gesture.vx < -0.5, gesture.vx),
      onPanResponderTerminate: () => settle(false),
    }),
  ).current

  const back = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponderCapture: (_, gesture) => horizontal(gesture, 1),
      onPanResponderMove: (_, gesture) => progress.setValue(clamp(1 - gesture.dx / live.current.width)),
      onPanResponderRelease: (_, gesture) => settle(!(gesture.dx > live.current.width / 3 || gesture.vx > 0.5), gesture.vx),
      onPanResponderTerminate: () => settle(true),
    }),
  ).current

  return (
    <DrawerContext value={() => settle(true)}>
      <View className="flex-1" {...edge.panHandlers}>
        {children}
      </View>
      <Drawer mounted={mounted} progress={progress} width={width} panHandlers={back.panHandlers} onClose={() => settle(false)} />
    </DrawerContext>
  )
}

interface NavLink {
  label: string
  icon: IconName
  /** A project's colour: a dot replaces the icon tile. */
  color?: string
  /** A private project: a lock follows the label. */
  locked?: boolean
  href: Href
  active: boolean
  badge?: number
  dot?: boolean
}

/**
 * The settings areas of the contextual drawer: the links of Shell.tsx's nav lists. A function, because
 * Shell.tsx imports this file: its arrays may not exist yet while this module loads.
 */
const areaNavs = () => ({ workspace: WORKSPACE_NAV, account: ACCOUNT_NAV, admin: ADMIN_NAV })

/** The web's `SideSheet`: the scrim and the sliding panel around `DrawerPanel`. */
function Drawer({
  mounted,
  progress,
  width,
  panHandlers,
  onClose,
}: {
  mounted: boolean
  progress: Animated.Value
  width: number
  panHandlers: ReturnType<typeof PanResponder.create>['panHandlers']
  onClose: () => void
}) {
  // the custom theme colours the drawer with its sidebar colour (the web's `appSidebarTheme`)
  const custom = useCustomTheme()
  const sidebarTheme = useMemo(() => (custom ? customSidebarTheme(custom) : null), [custom])
  const panel = <DrawerPanel mounted={mounted} width={width} onClose={onClose} />
  return (
    <Modal visible={mounted} transparent animationType="none" onRequestClose={onClose} statusBarTranslucent>
      <View className="flex-1" {...panHandlers}>
      <Animated.View style={{ position: 'absolute', inset: 0, backgroundColor: alpha(cssColors.black, 0.5), opacity: progress }}>
        <Pressable accessibilityLabel="Close menu" className="flex-1" onPress={onClose} />
      </Animated.View>
      {/* Animated: only the slide; the look is the web's classes on the inner view */}
      <Animated.View
        accessibilityLabel="Navigation"
        style={{ position: 'absolute', top: 0, bottom: 0, right: 0, width, transform: [{ translateX: progress.interpolate({ inputRange: [0, 1], outputRange: [width, 0] }) }] }}
      >
        {sidebarTheme ? (
          <ColorScope scheme={sidebarTheme.scheme} colors={sidebarTheme.colors}>
            {panel}
          </ColorScope>
        ) : (
          panel
        )}
      </Animated.View>
      </View>
    </Modal>
  )
}

/**
 * The web's `SidebarNav`: Search, the products, Manage. A settings page or Tasks shows its area's links and Back
 * instead of the app links; Back shows the app links again without leaving the page.
 */
function DrawerPanel({ mounted, width, onClose }: { mounted: boolean; width: number; onClose: () => void }) {
  const { colors } = useTheme()
  const { user, workspace } = useOrbit()
  const cloud = useAuthOptions().data?.billing_enabled ?? false
  const insets = useSafeAreaInsets()
  const pathname = usePathname()
  const { view, project } = useGlobalSearchParams<{ view?: string; project?: string }>()
  const chatBadge = useChatBadge()
  // a guest has Chat and its account settings only
  const guest = isGuestRole(workspace.role)
  const reduceMotion = useReduceMotion()

  const navs = areaNavs()
  const routeArea = drawerAreaOf(pathname, navs)
  const pageArea = guest && routeArea === 'tasks' ? null : routeArea
  // the area whose Back was pressed: the app links show until the page moves to another area or the drawer closes
  const [mainMenuFor, setMainMenuFor] = useState<DrawerArea | null>(null)
  if (!mounted && mainMenuFor) setMainMenuFor(null)
  // the area Settings or Admin opened in the drawer: its links show without leaving the page, like the web's sidebar
  const [openedArea, setOpenedArea] = useState<DrawerArea | null>(null)
  if (!mounted && openedArea) setOpenedArea(null)
  const area = openedArea ?? (pageArea && pageArea !== mainMenuFor ? pageArea : null)
  // Back shows the app links again, also over the area of the page that is open
  const backToApp = () => {
    setOpenedArea(null)
    setMainMenuFor(pageArea)
  }
  // Only a change while the drawer is open slides in: an area from the right, the app links back from the left.
  const content = area ?? 'app'
  const [shown, setShown] = useState(content)
  const [enter, setEnter] = useState<SlideFrom | null>(null)
  if (shown !== content) {
    setShown(content)
    setEnter(mounted ? (content === 'app' ? 'left' : 'right') : null)
  }
  if (!mounted && enter) setEnter(null)

  const go = (href: Href) => {
    onClose()
    router.navigate(href)
  }
  const onTasks = pathname === '/tasks' || pathname.startsWith('/tasks/')
  const onSettings = pathname === '/settings' || pathname.startsWith('/settings/')
  const onAdmin = pathname.startsWith('/settings/admin')
  const workspaceLinks: NavLink[] = [
    { label: 'Home', icon: 'home', href: '/', active: pathname === '/' },
    { label: 'Tasks', icon: 'tasks', href: tasksHref({ project }, {}), active: onTasks && !view },
  ]

  return (
    <View className="flex-1 overflow-hidden rounded-l-2xl border-l border-sidebar-edge bg-sidebar px-3" style={{ paddingTop: 12 + insets.top, paddingBottom: 12 + insets.bottom }}>
      <Wash color={colors.sidebarWash} width={width} />
      {/* the web's phone sheet: no workspace header or account button, the top bar has them */}
      {guest ? null : <View className="px-1 pb-3">
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Search"
          onPress={() => go('/tasks?search=1')}
          className="h-8 flex-row items-center gap-2.5 rounded-md bg-neutral-100 pr-1.5 pl-2.5 active:bg-neutral-200 dark:border dark:border-sidebar-tint/10 dark:bg-sidebar-tint/6 dark:active:bg-sidebar-tint/10"
        >
          <Icon name="search" size={16} color={alpha(colors.mutedForeground, 0.9)} />
          <RNText className="font-noto-medium text-[13px] text-muted-foreground">Search</RNText>
        </Pressable>
      </View>}

      <SlideIn key={content} from={reduceMotion ? null : enter}>
        <ScrollView className="-mx-1 flex-1" contentContainerClassName="grow gap-0.5 px-1 pb-2">
          {area === 'tasks' ? (
            <TasksArea view={view} project={project} onTasks={pathname === '/tasks'} onBack={backToApp} go={go} />
          ) : area ? (
            <>
              <BackRow label={DRAWER_AREA_TITLES[area]} onPress={backToApp} />
              {drawerSections(area, allowedNav(navs[area], workspace.permissions, cloud, guest)).map((section) => (
                <Fragment key={section.label}>
                  <Section label={section.label} />
                  {section.items.map((item) => (
                    <NavRow key={item.href} link={{ label: item.label, icon: item.icon, href: item.href, active: pathname === item.href }} onPress={() => go(item.href)} />
                  ))}
                </Fragment>
              ))}
            </>
          ) : guest ? (
            <>
              <NavRow link={{ label: 'Chat', icon: 'chat', href: '/chat', active: pathname.startsWith('/chat'), badge: chatBadge.count, dot: chatBadge.unread }} onPress={() => go('/chat')} />
              <NavRow link={{ label: 'Settings', icon: 'settings', href: guestSettings, active: onSettings && !onAdmin }} onPress={() => go(guestSettings)} />
              {user.installation_admin ? (
                <>
                  <View className="flex-1" />
                  <NavRow link={{ label: 'Admin', icon: 'admin', href: '/settings/admin', active: onAdmin }} tone="neutral" onPress={() => go('/settings/admin')} />
                </>
              ) : null}
            </>
          ) : (
            <>
              {workspaceLinks.map((link) => (
                <NavRow key={link.label} link={link} onPress={() => go(link.href)} />
              ))}
              <NavRow link={{ label: 'Docs', icon: 'docs', href: '/docs', active: pathname.startsWith('/docs') }} onPress={() => go('/docs')} />
              <NavRow link={{ label: 'Chat', icon: 'chat', href: '/chat', active: pathname.startsWith('/chat'), badge: chatBadge.count, dot: chatBadge.unread }} onPress={() => go('/chat')} />
              <NavRow link={{ label: 'Mail', icon: 'mail', href: '/', active: false }} comingSoon />
              <Section label="Manage" />
              <NavRow link={{ label: 'Settings', icon: 'settings', href: '/settings', active: onSettings && !onAdmin }} onPress={() => setOpenedArea('workspace')} />
              {user.installation_admin ? (
                <>
                  {/* the installation, not the workspace: it sits apart at the bottom */}
                  <View className="flex-1" />
                  <NavRow link={{ label: 'Admin', icon: 'admin', href: '/settings/admin', active: onAdmin }} tone="neutral" onPress={() => setOpenedArea('admin')} />
                </>
              ) : null}
            </>
          )}
        </ScrollView>
      </SlideIn>

    </View>
  )
}

/**
 * The Tasks area (the web's `TasksAreaNav`): All tasks, Views, the task presets and the projects. A preset
 * keeps the project and a project keeps the preset. The web's Favorite views are left out: the app has no saved views.
 */
function TasksArea({ view, project, onTasks, onBack, go }: { view?: string; project?: string; onTasks: boolean; onBack: () => void; go: (href: Href) => void }) {
  const projects = useProjects().data ?? []
  const connected = useGithubSettings(projects.length > 0).data?.connected_projects ?? []
  const [creating, setCreating] = useState(false)
  const current = { view, project }
  const row = (link: NavLink) => <NavRow key={String(link.href)} link={link} onPress={() => go(link.href)} />
  return (
    <>
      <BackRow label={DRAWER_AREA_TITLES.tasks} onPress={onBack} />
      {row({ label: 'All tasks', icon: 'tasks', href: tasksHref(current, { view: null }), active: onTasks && !view })}
      {/* the app's task list is its views page, like the main menu's Views row */}
      <NavRow link={{ label: 'Views', icon: 'layer', href: '/tasks', active: false }} onPress={() => go('/tasks')} />
      <Section label="My work" />
      {TASK_PRESETS.map((preset) => row({ label: preset.label, icon: preset.icon, href: tasksHref(current, { view: preset.view }), active: onTasks && view === preset.view }))}
      {projects.length > 0 ? (
        <>
          <Section label="Projects" />
          {row({ label: 'All projects', icon: 'folder', href: tasksHref(current, { project: null }), active: onTasks && !project })}
          {projects.map((item) => row({ label: item.name, icon: 'folder', color: item.color, locked: item.private, href: tasksHref(current, { project: item.id }), active: onTasks && project === item.id }))}
          {/* the open project's pull requests, when it has a GitHub repository (the web's hover link on the project) */}
          {project && connected.includes(project) ? row({ label: 'Pull requests', icon: 'code', href: `/tasks/project/${project}/pulls`, active: false }) : null}
        </>
      ) : <Section label="Projects" />}
      <NavRow link={{ label: 'New project', icon: 'plus', href: '/tasks', active: false }} onPress={() => setCreating(true)} />
      <NavRow link={{ label: 'Trash', icon: 'trash', href: '/tasks/trash', active: false }} onPress={() => go('/tasks/trash')} />
      <NewProjectSheet visible={creating} onClose={() => setCreating(false)} onCreated={(created) => {
        setCreating(false)
        go(tasksHref(current, { project: created.id }))
      }} />
    </>
  )
}

type SlideFrom = 'left' | 'right'

/** The web's `slide-in-from-left-6` / `slide-in-from-right-6` with a fade, 200ms. Mounted again for each new content. */
function SlideIn({ from, children }: { from: SlideFrom | null; children: ReactNode }) {
  const progress = useRef(new Animated.Value(from ? 0 : 1)).current
  useEffect(() => {
    if (from) Animated.timing(progress, { toValue: 1, duration: 200, easing: Easing.out(Easing.cubic), useNativeDriver: true }).start()
  }, [from, progress])
  const offset = from === 'left' ? -24 : 24
  return (
    <Animated.View className="flex-1" style={{ opacity: progress, transform: [{ translateX: progress.interpolate({ inputRange: [0, 1], outputRange: [offset, 0] }) }] }}>
      {children}
    </Animated.View>
  )
}

/**
 * Coolify's Back row (the web's `BackToApp`): a small chevron as wide as an icon tile, then the area's name, centred.
 * No tile, a fill while pressed.
 */
function BackRow({ label, onPress }: { label: string; onPress: () => void }) {
  const { colors, scheme } = useTheme()
  const flat = useButtonStyle() === 'flat'
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel="Back to the main menu"
      onPress={onPress}
      className={`mb-1 flex-row items-center gap-2.5 rounded-md pr-2.5 active:bg-neutral-100 dark:active:bg-white/6 ${flat ? 'h-8 pl-2.5' : 'h-9 pl-1'}`}
    >
      <View className={`h-4 items-center justify-center ${flat ? 'w-[18px]' : 'w-[30px]'}`}>
        <Icon name="chevronLeft" size={12} color={scheme === 'dark' ? colors.sidebarNav : colors.primary} />
      </View>
      <RNText numberOfLines={1} className="flex-1 pr-6 text-center font-noto-medium text-[13px] text-foreground">
        {label}
      </RNText>
    </Pressable>
  )
}

/**
 * The web `SidebarNavItem`: a 36px row with a 30px icon tile; the current row's tile turns pink. The Flat style has no
 * tile: an 18px icon on a 32px row, and the current row is filled.
 */
function NavRow({ link, onPress, comingSoon = false, tone = 'brand' }: { link: NavLink; onPress?: () => void; comingSoon?: boolean; tone?: TileTone }) {
  const { colors } = useTheme()
  const flat = useButtonStyle() === 'flat'
  const shape = flat ? `h-8 rounded-md pl-2.5 ${link.active ? 'bg-black/5 dark:bg-white/6' : 'active:bg-black/4 dark:active:bg-white/5'}` : 'h-9 pl-1'
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${link.label}${link.locked ? ', private' : ''}${link.badge ? `, ${link.badge} unread` : link.dot ? ', unread' : ''}`}
      accessibilityState={{ selected: link.active, disabled: comingSoon }}
      disabled={comingSoon || !onPress}
      onPress={onPress}
      className={`flex-row items-center gap-2.5 pr-2.5 ${shape} ${comingSoon ? 'opacity-[0.48]' : ''}`}
    >
      {({ pressed }) => (
        <>
          {link.color ? (
            // the web's `dotIcon`: the project's colour where the icon would be, as wide as the icon or tile
            <View className={`items-center justify-center ${flat ? 'w-[18px]' : 'w-[30px]'}`}>
              <View className="size-2.5 rounded-[3px]" style={{ backgroundColor: link.color }} />
            </View>
          ) : flat ? (
            <Icon name={link.icon} size={18} color={alpha(link.active || pressed ? colors.sidebarNavActive : colors.sidebarNav, 0.9)} />
          ) : (
            <Tile icon={link.icon} active={link.active} pressed={pressed} tone={tone} />
          )}
          <RNText numberOfLines={1} className={`flex-1 font-noto-medium text-[13px] ${link.active || pressed ? 'text-sidebar-nav-active' : 'text-sidebar-nav'}`}>
            {link.label}
          </RNText>
          {link.locked ? <Icon name="lock" size={12} color={alpha(colors.sidebarNav, 0.7)} /> : null}
          {comingSoon ? <RNText className="font-noto-medium text-[10px] text-muted-foreground/70">Coming soon</RNText> : null}
          {link.badge ? <CountBadge count={link.badge} /> : link.dot ? <View className="mr-1 size-2 rounded-full bg-primary" /> : null}
        </>
      )}
    </Pressable>
  )
}

/** `brand`: the workspace's apps, pink-tinted in dark mode. `neutral`: Admin, which is the installation, not the workspace. */
type TileTone = 'brand' | 'neutral'

/**
 * The web's `sidebarNavTileVariants`. The 2px ledge (`shadow-[0_2px_0_var(--tile-depth)]`) and the current tile's
 * `color-mix` border stay styles: they are computed colors.
 */
function Tile({ icon, active, pressed, tone }: { icon: IconName; active: boolean; pressed: boolean; tone: TileTone }) {
  const { colors, scheme } = useTheme()
  const dark = scheme === 'dark'
  const className = active
    ? 'bg-white border-primary/45 dark:bg-primary'
    : tone === 'brand'
      ? 'bg-sidebar border-black/9 dark:border-sidebar-tint/13 dark:bg-primary/7'
      : 'bg-sidebar border-black/9 dark:border-white/7 dark:bg-white/3'
  const edge = active
    ? dark ? mix(colors.primary, 0.6, cssColors.black) : mix(colors.primary, 0.45, cssColors.white)
    : dark ? alpha(cssColors.white, 0.05) : alpha(cssColors.black, 0.22)
  const icon_ = active ? (dark ? cssColors.white : colors.primary) : dark ? (tone === 'brand' ? colors.sidebarTintForeground : NEUTRAL_TILE_ICON_DARK) : colors.sidebarNav
  return (
    <View
      className={`size-[30px] items-center justify-center rounded-[10px] border ${className}`}
      style={{
        // the web's tile borders; set here because Uniwind does not let `dark:border-*` win over `border-black/9`
        borderColor: active
          ? dark ? mix(colors.primary, 0.75, cssColors.white) : alpha(colors.primary, 0.45)
          : !dark ? alpha(cssColors.black, 0.09) : tone === 'brand' ? alpha(colors.sidebarTint, 0.13) : alpha(cssColors.white, 0.07),
        boxShadow: pressed ? undefined : `0 2px 0 ${edge}`,
        transform: [{ translateY: pressed ? 2 : 0 }],
      }}
    >
      <Icon name={icon} size={16} color={icon_} />
    </View>
  )
}

/** The web's section label. */
function Section({ label }: { label: string }) {
  const { scheme } = useTheme()
  // The web's header: Title Case in light, small mono capitals in dark (0.08em of 10.5px).
  const face = scheme === 'dark' ? 'font-mono text-[10.5px] tracking-[0.84px] uppercase' : 'font-noto-medium text-[11px]'
  return <RNText className={`mt-3 px-2.5 py-1 leading-4 text-sidebar-nav-muted ${face}`}>{label}</RNText>
}

/** The web's `oklch(0.72 0 0)` neutral tile icon (dark). */
const NEUTRAL_TILE_ICON_DARK = '#a4a4a4'

/** The web sidebar's `bg-linear-to-b from-sidebar-wash to-transparent to-[520px]`: a pink wash fading out from the top. */
function Wash({ color, width }: { color: string; width: number }) {
  const wash = splitAlpha(color)
  return (
    <Svg pointerEvents="none" width={width} height={520} style={{ position: 'absolute', top: 0, left: 0 }}>
      <Defs>
        <LinearGradient id="sidebar-wash" x1="0" y1="0" x2="0" y2="1">
          <Stop offset="0" stopColor={wash.color} stopOpacity={wash.opacity} />
          <Stop offset="1" stopColor={wash.color} stopOpacity={0} />
        </LinearGradient>
      </Defs>
      <Rect width="100%" height="100%" fill="url(#sidebar-wash)" />
    </Svg>
  )
}
