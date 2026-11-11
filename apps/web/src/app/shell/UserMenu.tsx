import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Brush, ChevronDown, Logout as LogOut, User } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useCurrentUser, useLogout } from '@/features/auth/api'
import { userColor } from '@/features/workspaces/api'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Tip } from '@/components/common/Tip'
import { CustomStatusDialog } from '@/features/realtime/components/CustomStatusDialog'
import { CustomStatusText } from '@/features/realtime/components/CustomStatusText'
import { StatusMenuItems } from '@/features/realtime/components/StatusMenuItems'
import { useOwnStatus } from '@/features/realtime/useOwnStatus'

/** Signed-in profile and account actions shared by both sidebars. */
export function UserMenu({ collapsed = false, topbar = false }: { collapsed?: boolean; /** The phone top bar's look: avatar and chevron, the menu opens below. */ topbar?: boolean }) {
  const user = useCurrentUser()
  const logout = useLogout()
  const navigate = useNavigate()
  const me = user.data
  // the user's own status is what they chose, not what the others see
  const status = useOwnStatus()
  const hasCustomStatus = status.emoji !== null || status.text !== null
  const [editingStatus, setEditingStatus] = useState(false)

  const userName = me?.display_name ?? 'Account'

  return (
    <>
      <DropdownMenu>
        <Tip label={collapsed ? userName : undefined} side="right">
          <DropdownMenuTrigger
            render={
              <Button
                variant="outline"
                className={cn('h-8 w-full min-w-0 flex-1 justify-start gap-2 rounded-lg px-2 aria-expanded:border-primary aria-expanded:bg-primary/5 dark:border-sidebar-tint/24 dark:bg-primary/14 dark:text-[oklch(0.95_0.02_359)] dark:[--button-depth:color-mix(in_oklab,var(--primary)_35%,transparent)] dark:hover:border-sidebar-tint/34 dark:hover:bg-primary/18 dark:aria-expanded:border-primary dark:aria-expanded:bg-primary/18 dark:[&_svg]:text-sidebar-tint', collapsed && 'w-8 flex-none justify-center px-0', topbar && 'h-9 w-auto flex-none gap-1.5')}
                aria-label={`Account menu for ${userName}`}
              />
            }
          >
            <UserAvatar
              user={me ? { name: me.display_name || me.email, color: userColor(me.id), avatarUrl: me.avatar_url } : null}
              name={userName}
              size={topbar ? 24 : 20}
              status={me ? status.presence : undefined}
              className="shrink-0"
            />
            {!collapsed && !topbar ? (
              <span className="flex min-w-0 flex-1 text-left">
                <span className="min-w-0 truncate text-xs font-medium text-foreground">{userName}</span>
              </span>
            ) : null}
            {!collapsed ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground/70 transition-transform group-aria-expanded/button:rotate-180" /> : null}
          </DropdownMenuTrigger>
        </Tip>
        {/* the pink tint of the sidebar it opens from */}
        <DropdownMenuContent side={topbar ? 'bottom' : 'top'} align={topbar ? 'end' : 'start'} sideOffset={4} className="w-52 max-w-[calc(100vw-32px)] rounded-[10px] dark:bg-sidebar-popover dark:ring-sidebar-tint/22 dark:**:data-[slot$=-item]:focus:bg-sidebar-tint/10 dark:**:data-[slot$=-item]:data-highlighted:bg-sidebar-tint/10">
          <div className="min-w-0 px-2 py-1.5">
            <div className="truncate text-[0.8125rem] font-semibold text-foreground">{userName}</div>
            {hasCustomStatus ? (
              <div className="truncate text-[0.6875rem] text-muted-foreground">
                <CustomStatusText emoji={status.emoji} text={status.text} />
              </div>
            ) : (
              <div className="truncate text-[0.6875rem] text-muted-foreground/70">{me?.email}</div>
            )}
          </div>
          <DropdownMenuSeparator />
          {me ? (
            <>
              <StatusMenuItems onEditCustom={() => setEditingStatus(true)} />
              <DropdownMenuSeparator />
            </>
          ) : null}
          <DropdownMenuItem onClick={() => navigate('/profile')}>
            <User />
            Account settings
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => navigate('/profile/appearance')}>
            <Brush />
            Appearance
          </DropdownMenuItem>
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
      {editingStatus ? <CustomStatusDialog onClose={() => setEditingStatus(false)} /> : null}
    </>
  )
}
