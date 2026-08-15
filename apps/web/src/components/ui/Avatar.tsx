import type { User } from '../../mock/types'

interface AvatarProps {
  user: User | null | undefined
  size?: number
  showOnline?: boolean
  /** Fallback initials when there is no user (e.g. external senders). */
  name?: string
}

export function Avatar({ user, size = 24, showOnline = false, name }: AvatarProps) {
  const label = user?.name ?? name ?? '?'
  const initials = label
    .split(' ')
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? '')
    .join('')
  return (
    <span
      className="avatar"
      data-online={showOnline && user?.online ? 'true' : undefined}
      style={{
        width: size,
        height: size,
        fontSize: Math.max(9, Math.round(size * 0.38)),
        background: user ? `color-mix(in srgb, ${user.color} 22%, transparent)` : undefined,
        color: user?.color,
      }}
      title={label}
    >
      {initials}
    </span>
  )
}
