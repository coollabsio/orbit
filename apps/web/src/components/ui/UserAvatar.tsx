import type { CSSProperties } from 'react'
import type { User } from '../../mock/types'
import { Avatar, AvatarBadge, AvatarFallback, AvatarGroup, AvatarGroupCount } from './avatar'
import { cn } from 'cn'

interface UserAvatarProps {
  user: User | null | undefined
  size?: number
  showOnline?: boolean
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

export function UserAvatar({ user, size = 24, showOnline = false, name, className }: UserAvatarProps) {
  const label = user?.name ?? name ?? '?'
  const fallbackStyle: CSSProperties = user
    ? { background: `color-mix(in srgb, ${user.color} 22%, transparent)`, color: user.color }
    : {}
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
      {showOnline && user?.online ? (
        <AvatarBadge className="size-2 bg-green-500 ring-background" />
      ) : null}
    </Avatar>
  )
}

/** Overlapping avatars for several users; an empty dash avatar when there is nobody. */
export function UserAvatarStack({ users, size = 18, max = 3 }: { users: User[]; size?: number; max?: number }) {
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
