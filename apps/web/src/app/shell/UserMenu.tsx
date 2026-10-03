import { useState } from 'react'
import { useNavigate } from 'react-router'
import { ChevronDown, Logout as LogOut, Setting2 as Settings, User } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useTheme, type Theme } from '@/lib/themeContext'
import { useCurrentUser, useLogout } from '@/features/auth/api'
import { userColor } from '@/features/workspaces/api'
import { UserAvatar } from '@/components/common/UserAvatar'

const THEMES: { value: Theme; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
]

/** Signed-in profile and account actions shared by both sidebars. */
export function UserMenu({ collapsed = false }: { collapsed?: boolean }) {
  const user = useCurrentUser()
  const logout = useLogout()
  const navigate = useNavigate()
  const { theme, setTheme } = useTheme()
  const me = user.data
  const [appearanceOpen, setAppearanceOpen] = useState(false)

  const userName = me?.display_name ?? 'Account'

  return (
    <DropdownMenu onOpenChange={(open) => { if (!open) setAppearanceOpen(false) }}>
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            className={cn('h-auto min-h-[34px] w-full min-w-0 flex-1 justify-start gap-1.5 p-1.5 font-normal', collapsed && 'w-8 px-[5px]')}
            title={userName}
            aria-label={`Account menu for ${userName}`}
          />
        }
      >
        <UserAvatar
          user={me ? { name: me.display_name || me.email, color: userColor(me.id), avatarUrl: me.avatar_url } : null}
          name={userName}
          size={20}
          className="shrink-0"
        />
        {!collapsed ? (
          <span className="flex min-w-0 flex-1 text-left">
            <span className="min-w-0 truncate text-xs font-medium text-foreground">{userName}</span>
          </span>
        ) : null}
        {!collapsed ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground/70 transition-transform group-aria-expanded/button:rotate-180" /> : null}
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-[220px] max-w-[calc(100vw-32px)]">
        <div className="min-w-0 px-2 py-1.5">
          <div className="truncate text-[13px] font-semibold text-foreground">{userName}</div>
          <div className="truncate text-[11px] text-muted-foreground/70">{me?.email}</div>
        </div>
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={() => navigate('/profile')}>
          <User />
          Account settings
        </DropdownMenuItem>
        <DropdownMenuItem closeOnClick={false} aria-expanded={appearanceOpen} onClick={() => setAppearanceOpen((o) => !o)}>
          <Settings />
          Appearance
          <ChevronDown className={cn('ml-auto size-3.5 text-muted-foreground transition-transform', appearanceOpen && 'rotate-180')} />
        </DropdownMenuItem>
        {appearanceOpen ? (
          <DropdownMenuRadioGroup className="pl-6" value={theme} onValueChange={(value) => setTheme(value as Theme)}>
            {THEMES.map((option) => (
              <DropdownMenuRadioItem key={option.value} value={option.value} closeOnClick>
                {option.label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        ) : null}
        <DropdownMenuSeparator />
        {/* data-danger, not variant="destructive": the preset menu popup forces destructive items to the accent color */}
        <DropdownMenuItem
          className="data-[danger=true]:text-destructive data-[danger=true]:focus:bg-destructive/10 data-[danger=true]:focus:text-destructive data-[danger=true]:focus:**:text-destructive"
          data-danger="true"
          disabled={logout.isPending}
          closeOnClick={false}
          onClick={() => logout.mutate(undefined, { onSuccess: () => navigate('/login', { replace: true }) })}
        >
          <LogOut />
          {logout.isPending ? 'Logging out…' : 'Log out'}
        </DropdownMenuItem>
        {logout.isError ? <p className="px-2 py-1.5 text-xs text-destructive" role="alert">Could not log out. Please try again.</p> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
