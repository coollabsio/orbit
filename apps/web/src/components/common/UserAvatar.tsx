import type { CSSProperties } from 'react'
import { Avatar, AvatarBadge, AvatarFallback, AvatarGroup, AvatarGroupCount, AvatarImage } from '@/components/ui/avatar'
import { cn } from 'cn'
import { type MemberStatus, statusLabel } from './memberStatus'
import { StatusMark } from './StatusMark'

/** What the avatar needs of a person. A workspace member and a mail contact both fit. */
export interface AvatarPerson {
  name: string
  color: string
  avatarUrl?: string | null
}

interface UserAvatarProps {
  user: AvatarPerson | null | undefined
  size?: number
  /** Presence from the caller (`usePresenceOf`, or the user's own choice); no badge without it. */
  status?: MemberStatus
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

/** The presence badge of an avatar: 8px on avatars up to 24px, 10px up to 40px, 14px from 64px. Goes inside an `Avatar`. */
export function AvatarStatusBadge({ status, size }: { status: MemberStatus; size: number }) {
  const badge = size <= 24 ? 8 : size <= 40 ? 10 : size < 64 ? 12 : 14
  return (
    <AvatarBadge className="bg-background ring-background" style={{ width: badge, height: badge }}>
      <StatusMark status={status} size={badge} />
      <span className="sr-only">{statusLabel(status)}</span>
    </AvatarBadge>
  )
}

export function UserAvatar({ user, size = 24, status, name, className }: UserAvatarProps) {
  const label = user?.name ?? name ?? '?'
  const fallbackStyle: CSSProperties = user
    ? // the tint sits on the theme's muted surface and the initials lean toward its text colour: readable in both themes
      { background: `color-mix(in oklch, ${user.color} 26%, var(--muted))`, color: `color-mix(in oklch, ${user.color} 60%, var(--foreground))` }
    : {}
  return (
    <Avatar
      className={cn('after:border-transparent', className)}
      style={{ width: size, height: size }}
      title={label}
    >
      {user?.avatarUrl ? <AvatarImage src={user.avatarUrl} alt="" /> : null}
      <AvatarFallback
        className="font-semibold"
        style={{ fontSize: Math.max(9, Math.round(size * 0.38)), ...fallbackStyle }}
      >
        {initialsOf(label)}
      </AvatarFallback>
      {status ? <AvatarStatusBadge status={status} size={size} /> : null}
    </Avatar>
  )
}

/** Overlapping avatars for several users; an empty dash avatar when there is nobody. */
export function UserAvatarStack({ users, size = 18, max = 3 }: { users: Array<AvatarPerson & { id: string }>; size?: number; max?: number }) {
  if (users.length === 0) return <UserAvatar user={undefined} size={size} name="—" />
  const shown = users.slice(0, max)
  const rest = users.length - shown.length
  return (
    <AvatarGroup className="*:data-[slot=avatar]:ring-1 *:data-[slot=avatar]:ring-card" title={users.map((u) => u.name).join(', ')}>
      {shown.map((u) => (
        <UserAvatar key={u.id} user={u} size={size} />
      ))}
      {rest > 0 ? (
        <AvatarGroupCount className="ring-1 ring-card" style={{ width: size, height: size, fontSize: Math.max(9, Math.round(size * 0.38)) }}>
          +{rest}
        </AvatarGroupCount>
      ) : null}
    </AvatarGroup>
  )
}
