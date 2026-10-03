import { useState } from 'react'
import { useNavigate } from 'react-router'
import { User } from 'reicon-react'
import { statusLabel } from '@/components/common/memberStatus'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useCurrentUser } from '@/features/auth/api'
import { CustomStatusDialog } from '@/features/realtime/components/CustomStatusDialog'
import { CustomStatusText } from '@/features/realtime/components/CustomStatusText'
import { StatusMenuItems } from '@/features/realtime/components/StatusMenuItems'
import { useOwnStatus } from '@/features/realtime/useOwnStatus'
import { userColor } from '@/features/workspaces/api'

/**
 * The foot of the chat sidebar: the signed-in user with their own status (what they chose, so "Invisible" shows as
 * such). The whole row opens the status menu.
 */
export function ChatUserBar() {
  const me = useCurrentUser().data
  const status = useOwnStatus()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  if (!me) return null
  const name = me.display_name || me.email
  const hasCustom = status.emoji !== null || status.text !== null

  return (
    <div data-slot="chat-user-bar" className="flex h-12 shrink-0 items-center border-t px-2">
      <DropdownMenu>
        <DropdownMenuTrigger
          render={<Button type="button" variant="ghost" className="h-9 w-full min-w-0 justify-start gap-2 px-1.5 font-normal transition-none" aria-label={`Status menu for ${name}`} />}
        >
          <UserAvatar user={{ name, color: userColor(me.id), avatarUrl: me.avatar_url }} size={28} status={status.presence} />
          <span className="flex min-w-0 flex-1 flex-col text-left leading-tight">
            <span className="truncate text-[13px] font-medium text-foreground">{name}</span>
            <span className="truncate text-[11px] text-muted-foreground">
              {hasCustom ? <CustomStatusText emoji={status.emoji} text={status.text} /> : statusLabel(status.presence)}
            </span>
          </span>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="start" className="min-w-56 max-w-[calc(100vw-16px)]">
          <StatusMenuItems onEditCustom={() => setEditing(true)} />
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={() => navigate('/profile')}>
            <User />
            Account settings
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {editing ? <CustomStatusDialog onClose={() => setEditing(false)} /> : null}
    </div>
  )
}
