import type { CSSProperties } from 'react'
import { Avatar, AvatarBadge, AvatarFallback, AvatarGroup, AvatarGroupCount } from '@/components/ui/avatar'
import { cn } from 'cn'

/** What the avatar needs of a person. A workspace member and a mail contact both fit. */
export interface AvatarPerson {
  name: string
  color: string
  online?: boolean
}

interface UserAvatarProps {
  user: AvatarPerson | null | undefined
  size?: number
  showOnline?: boolean
  /** Presence from the caller (chat keeps it in its own store); wins over `user.online`. */
  online?: boolean
  /** Fallback initials when there is no user (e.g. external senders). */
  name?: string
  className?: string
}

function initialsOf(label: string) {
  return label
    .split(' ')
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? '')
    .join('')
}

export function UserAvatar({ user, size = 24, showOnline = false, online, name, className }: UserAvatarProps) {
  const label = user?.name ?? name ?? '?'
  const fallbackStyle: CSSProperties = user
    ? // the tint sits on the theme's muted surface and the initials lean toward its text colour: readable in both themes
      { background: `color-mix(in oklch, ${user.color} 26%, var(--muted))`, color: `color-mix(in oklch, ${user.color} 60%, var(--foreground))` }
    : {}
  const presence = online ?? user?.online
  return (
    <Avatar
      className={cn('after:border-transparent', className)}
      style={{ width: size, height: size }}
      title={label}
    >
      <AvatarFallback
        className="font-semibold"
        style={{ fontSize: Math.max(9, Math.round(size * 0.38)), ...fallbackStyle }}
      >
        {initialsOf(label)}
      </AvatarFallback>
      {showOnline && presence !== undefined ? (
        // online is filled, offline is a hollow ring: the state does not depend on colour alone
        <AvatarBadge
          data-online={presence}
          className="size-2 border-[1.5px] border-muted-foreground bg-background ring-background data-[online=true]:border-green-500 data-[online=true]:bg-green-500"
        >
          <span className="sr-only">{presence ? 'Online' : 'Offline'}</span>
        </AvatarBadge>
      ) : null}
    </Avatar>
  )
}

/** Overlapping avatars for several users; an empty dash avatar when there is nobody. */
export function UserAvatarStack({ users, size = 18, max = 3 }: { users: Array<AvatarPerson & { id: string }>; size?: number; max?: number }) {
  if (users.length === 0) return <UserAvatar user={undefined} size={size} name="—" />
  const shown = users.slice(0, max)
  const rest = users.length - shown.length
  return (
    <AvatarGroup title={users.map((u) => u.name).join(', ')}>
      {shown.map((u) => (
        <UserAvatar key={u.id} user={u} size={size} />
      ))}
      {rest > 0 ? (
        <AvatarGroupCount style={{ width: size, height: size, fontSize: Math.max(9, Math.round(size * 0.38)) }}>
          +{rest}
        </AvatarGroupCount>
      ) : null}
    </AvatarGroup>
  )
}
