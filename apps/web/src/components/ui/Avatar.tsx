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

/** Overlapping avatars for several users; an empty dash avatar when there is nobody. */
export function AvatarStack({ users, size = 18, max = 3 }: { users: User[]; size?: number; max?: number }) {
  if (users.length === 0) return <Avatar user={undefined} size={size} name="—" />
  const shown = users.slice(0, max)
  const rest = users.length - shown.length
  return (
    <span className="avatar-stack" title={users.map((u) => u.name).join(', ')}>
      {shown.map((u) => (
        <Avatar key={u.id} user={u} size={size} />
      ))}
      {rest > 0 ? (
        <span className="avatar avatar-stack-more" style={{ width: size, height: size, fontSize: Math.max(9, Math.round(size * 0.38)) }}>
          +{rest}
        </span>
      ) : null}
    </span>
  )
}
