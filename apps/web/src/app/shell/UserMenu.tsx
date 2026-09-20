import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Check, ChevronDown, Logout as LogOut, Setting2 as Settings, User } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useTheme, type Theme } from '@/lib/themeContext'
import { useCurrentUser, useLogout } from '@/features/auth/api'

const THEMES: { value: Theme; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
]

// data-danger (not variant="destructive"): the preset menu popup forces destructive items to the accent color.
const optionClass =
  'min-h-11 w-full justify-between gap-2 rounded-md px-2 py-1.5 text-left text-sm font-normal text-foreground focus:bg-accent focus:text-accent-foreground data-disabled:opacity-40 data-[danger=true]:text-destructive data-[danger=true]:focus:bg-destructive/10 data-[danger=true]:focus:text-destructive data-[danger=true]:focus:**:text-destructive'

/** Signed-in profile and account actions shared by both sidebars. */
export function UserMenu({ collapsed = false }: { collapsed?: boolean }) {
  const user = useCurrentUser()
  const logout = useLogout()
  const navigate = useNavigate()
  const { theme, setTheme } = useTheme()
  const me = user.data
  const [appearanceOpen, setAppearanceOpen] = useState(false)

  const userName = me?.display_name ?? 'Account'
  const initial = (me?.display_name || me?.email || 'A').charAt(0).toUpperCase()

  return (
    <DropdownMenu onOpenChange={(open) => { if (!open) setAppearanceOpen(false) }}>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            className={cn(
              'h-auto min-h-[34px] w-full min-w-0 flex-1 justify-start gap-1.5 rounded-lg border border-border bg-card p-1.5 font-normal shadow-[0_1px_2px_rgba(0,0,0,0.05)] hover:bg-muted aria-expanded:bg-card aria-expanded:hover:bg-muted dark:hover:bg-muted',
              collapsed && 'w-8 px-[5px]',
            )}
            title={userName}
            aria-label={`Account menu for ${userName}`}
          />
        }
      >
        <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-[11px] font-semibold text-foreground">{initial}</span>
        {!collapsed ? (
          <span className="flex min-w-0 flex-1 text-left">
            <span className="min-w-0 truncate text-xs font-medium text-foreground">{userName}</span>
          </span>
        ) : null}
        {!collapsed ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground/70 transition-transform group-aria-expanded/button:rotate-180" /> : null}
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-[220px] max-w-[calc(100vw-32px)] p-1">
        <div className="min-w-0 px-2 py-1.5">
          <div className="truncate text-[13px] font-semibold text-foreground">{userName}</div>
          <div className="truncate text-[11px] text-muted-foreground/70">{me?.email}</div>
        </div>
        <DropdownMenuSeparator />
        <DropdownMenuItem className={optionClass} onClick={() => navigate('/profile')}>
          <span className="flex items-center gap-2">
            <User className="size-4 opacity-80" />
            Account settings
          </span>
        </DropdownMenuItem>
        <DropdownMenuItem
          className={optionClass}
          closeOnClick={false}
          aria-expanded={appearanceOpen}
          onClick={() => setAppearanceOpen((o) => !o)}
        >
          <span className="flex items-center gap-2">
            <Settings className="size-4 opacity-80" />
            Appearance
          </span>
          <ChevronDown className={cn('size-3.5 shrink-0 text-muted-foreground/70 transition-transform', appearanceOpen && 'rotate-180')} />
        </DropdownMenuItem>
        {appearanceOpen ? (
          <div className="mx-1 grid gap-0.5 pb-1 pl-6">
            {THEMES.map((option) => (
              <DropdownMenuItem
                key={option.value}
                className="min-h-8 w-full justify-between gap-2 rounded-md px-2 text-xs font-normal text-muted-foreground focus:bg-accent focus:text-foreground"
                onClick={() => setTheme(option.value)}
              >
                <span>{option.label}</span>
                {theme === option.value ? <Check className="size-3.5 text-primary" /> : null}
              </DropdownMenuItem>
            ))}
          </div>
        ) : null}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          className={optionClass}
          data-danger="true"
          disabled={logout.isPending}
          closeOnClick={false}
          onClick={() => logout.mutate(undefined, { onSuccess: () => navigate('/login', { replace: true }) })}
        >
          <span className="flex items-center gap-2">
            <LogOut className="size-4 opacity-90" />
            {logout.isPending ? 'Logging out…' : 'Log out'}
          </span>
        </DropdownMenuItem>
        {logout.isError ? <p className="px-2 py-1.5 text-xs text-destructive" role="alert">Could not log out. Please try again.</p> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
