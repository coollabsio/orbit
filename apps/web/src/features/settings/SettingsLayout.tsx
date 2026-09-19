import { NavLink, Outlet } from 'react-router'
import { Key, Settings, ShieldCheck, TriangleAlert, Trash2, Users } from 'lucide-react'
import { cn } from 'cn'
import { useWorkspace } from '../workspaces/workspaceContext'

interface NavItem {
  to: string
  label: string
  icon: React.ComponentType<{ className?: string }>
  end?: boolean
}

const SECTIONS: { label: string; items: NavItem[] }[] = [
  {
    label: 'Configuration',
    items: [
      { to: '/settings', label: 'General', icon: Settings, end: true },
      { to: '/settings/sessions', label: 'Sessions', icon: ShieldCheck },
      { to: '/settings/api-tokens', label: 'API tokens', icon: Key },
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
    items: [{ to: '/settings/danger-zone', label: 'Danger zone', icon: TriangleAlert }],
  },
]

/** Coolify `x-settings.layout`: sticky sub-navigation (210px) + content column. */
export function SettingsLayout() {
  const { workspace } = useWorkspace()
  const canManageTokens = workspace.role === 'owner' || workspace.role === 'admin'
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <div className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
        {/* pane-header product-pane-header settings-page-header — hidden on mobile so the shell topbar takes over */}
        <div className="flex min-h-12 shrink-0 items-center gap-[7px] border-b border-border bg-background py-2 pr-3 pl-[19px] text-muted-foreground max-[899px]:hidden">
          <Settings className="size-[15px] shrink-0" />
          <span className="truncate text-[13px] font-semibold">Settings</span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <section className="grid w-full min-w-0 grid-cols-1 gap-6 px-5 pt-5 pb-8 min-[900px]:px-8 min-[900px]:pt-6 min-[900px]:pb-10 xl:grid-cols-[210px_minmax(0,1fr)] xl:gap-8 xl:px-10 xl:pt-7">
            <aside className="min-w-0 xl:sticky xl:top-7 xl:max-h-[calc(100dvh-5.5rem)] xl:self-start xl:overflow-x-hidden xl:overflow-y-auto xl:pr-1.5 xl:[overscroll-behavior:contain]">
              <nav
                aria-label="Settings"
                className="grid grid-cols-2 gap-0.5 border-y border-border py-3 min-[900px]:grid-cols-4 xl:grid-cols-1 xl:border-0 xl:py-0"
              >
                {SECTIONS.map((section, index) => (
                  <div key={section.label} className="contents">
                    <div
                      className={cn(
                        'col-span-full px-2.5 py-1 text-[11px] font-medium text-sidebar-foreground/60 select-none max-xl:hidden',
                        index > 0 && 'xl:mt-5 xl:border-t xl:border-border xl:pt-4',
                      )}
                    >
                      {section.label}
                    </div>
                    {section.items.filter((item) => item.to !== '/settings/api-tokens' || canManageTokens).map((item) => (
                      <NavLink
                        key={item.to}
                        to={item.to}
                        end={item.end}
                        className={({ isActive }) =>
                          cn(
                            'relative flex h-8 w-full min-w-0 items-center gap-2.5 overflow-hidden rounded-md px-2.5 text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-accent-foreground',
                            isActive && 'bg-sidebar-accent text-sidebar-accent-foreground',
                          )
                        }
                      >
                        <item.icon className="size-4.5 shrink-0 opacity-90" />
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
      </div>
    </div>
  )
}
