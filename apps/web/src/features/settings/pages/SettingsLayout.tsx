import { NavLink, Outlet } from 'react-router'
import { Driver, Key, Keyboard, Link2, Notification as Bell, Tag, Setting2 as Settings, ShieldTick as ShieldCheck, SmileCircle, Sticker, Danger as TriangleAlert, Trash as Trash2, User, People as Users } from 'reicon-react'
import { cn } from 'cn'
import { buttonVariants } from '@/components/ui/button'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import type { Permission } from '@/api/generated/types.gen'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

interface NavItem {
  to: string
  label: string
  icon: React.ComponentType<{ className?: string }>
  end?: boolean
  /** Shown only to roles that hold this permission. */
  permission?: Permission
}

const SECTIONS: { label: string; items: NavItem[] }[] = [
  {
    label: 'Configuration',
    items: [
      { to: '/settings', label: 'General', icon: Settings, end: true },
      { to: '/settings/api-tokens', label: 'API tokens', icon: Key, permission: 'api_tokens.manage' },
      { to: '/settings/labels', label: 'Labels', icon: Tag },
      { to: '/settings/emoji', label: 'Emoji', icon: SmileCircle, permission: 'chat.manage' },
      { to: '/settings/stickers', label: 'Stickers', icon: Sticker, permission: 'chat.manage' },
      { to: '/settings/github', label: 'GitHub', icon: Link2 },
      { to: '/tasks-trash', label: 'Trash', icon: Trash2 },
    ],
  },
  {
    label: 'Team',
    items: [
      { to: '/settings/members', label: 'Members', icon: Users },
    ],
  },
  {
    label: 'Workspace',
    items: [{ to: '/settings/danger-zone', label: 'Danger zone', icon: TriangleAlert, permission: 'workspace.delete' }],
  },
]

/** What belongs to the signed-in person, in every workspace. */
const ACCOUNT_SECTIONS: { label: string; items: NavItem[] }[] = [
  {
    label: 'Account',
    items: [
      { to: '/profile', label: 'Profile', icon: User, end: true },
      { to: '/profile/notifications', label: 'Notifications', icon: Bell },
      { to: '/profile/sessions', label: 'Sessions', icon: ShieldCheck },
      { to: '/profile/shortcuts', label: 'Keyboard shortcuts', icon: Keyboard },
      { to: '/profile/storage', label: 'Storage', icon: Driver },
    ],
  },
]

/** The workspace's settings. */
export function SettingsLayout() {
  const { workspace } = useWorkspace()
  const sections = SECTIONS
    .map((section) => ({ ...section, items: section.items.filter((item) => !item.permission || workspace.permissions.includes(item.permission)) }))
    .filter((section) => section.items.length > 0)
  return <SettingsShell title="Settings" icon={Settings} sections={sections} />
}

/** The signed-in person's own settings. */
export function AccountLayout() {
  return <SettingsShell title="Account settings" icon={User} sections={ACCOUNT_SECTIONS} />
}

/** Coolify `x-settings.layout`: sticky sub-navigation (210px) + content column. */
function SettingsShell({ title, icon: Icon, sections }: { title: string; icon: NavItem['icon']; sections: { label: string; items: NavItem[] }[] }) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane className="relative">
        {/* hidden on mobile so the shell topbar takes over */}
        <PaneHeader className="gap-[7px] bg-background pl-[19px] text-muted-foreground max-[899px]:hidden">
          <Icon className="size-[15px] shrink-0" />
          <PaneTitle className="text-muted-foreground">{title}</PaneTitle>
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <section className="grid w-full min-w-0 grid-cols-1 gap-6 px-5 pt-5 pb-8 min-[900px]:px-8 min-[900px]:pt-6 min-[900px]:pb-10 xl:grid-cols-[210px_minmax(0,1fr)] xl:gap-8 xl:px-10 xl:pt-7">
            <aside className="min-w-0 xl:sticky xl:top-7 xl:max-h-[calc(100dvh-5.5rem)] xl:self-start xl:overflow-x-hidden xl:overflow-y-auto xl:pr-1.5 xl:[overscroll-behavior:contain]">
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
            <div className="flex min-w-0 flex-col gap-6">
              <Outlet />
            </div>
          </section>
        </div>
      </Pane>
    </div>
  )
}
