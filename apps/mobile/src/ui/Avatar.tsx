import { View } from 'react-native'

import { avatarColors, userColor } from '../data/color'
import { presenceColors, useTheme } from '../theme'
import { Text } from './Text'

export type Presence = 'online' | 'idle' | 'dnd' | 'offline' | 'invisible'

const initialsOf = (name: string) =>
  name
    .split(' ')
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? '')
    .join('')

export interface AvatarProps {
  /** The user id: it picks the color, as on the web. */
  id: string
  name: string
  size?: number
  /** A presence badge in the corner. None without it. */
  presence?: Presence
  /** The color behind the avatar: the badge's ring and cut-outs take it. */
  surface?: string
}

/** The web `UserAvatar` fallback: initials on a tint of the person's color over the muted surface. */
export function Avatar({ id, name, size = 24, presence, surface }: AvatarProps) {
  const { colors } = useTheme()
  const look = avatarColors(userColor(id), colors.muted, colors.foreground)
  return (
    <View accessibilityLabel={name} style={{ width: size, height: size }}>
      <View className="items-center justify-center overflow-hidden rounded-full" style={{ width: size, height: size, backgroundColor: look.background }}>
        <Text className="font-noto-semibold" style={{ color: look.text, fontSize: Math.max(9, Math.round(size * 0.38)), lineHeight: size }}>
          {initialsOf(name)}
        </Text>
      </View>
      {presence ? <PresenceBadge presence={presence} size={size <= 24 ? 8 : size <= 40 ? 10 : size < 64 ? 12 : 14} surface={surface ?? colors.background} /> : null}
    </View>
  )
}

/** The web `AvatarBadge` + `StatusMark`: bottom-right, with a 2px ring of the background around it. Sizes and colors are props. */
function PresenceBadge({ presence, size, surface }: { presence: Presence; size: number; surface: string }) {
  return (
    <View className="absolute -right-0.5 -bottom-0.5 rounded-full p-0.5" style={{ backgroundColor: surface }}>
      <StatusMark presence={presence} size={size} surface={surface} />
    </View>
  )
}

/** Each presence has its own shape: online a dot, idle a crescent, do not disturb a dot with a bar, offline a ring. */
export function StatusMark({ presence, size, surface }: { presence: Presence; size: number; surface: string }) {
  const filled = presence === 'online' || presence === 'idle' || presence === 'dnd'
  return (
    <View
      className={`items-center justify-center overflow-hidden rounded-full ${filled ? '' : 'border-[1.5px] border-muted-foreground'}`}
      style={{ width: size, height: size, backgroundColor: filled ? presenceColors[presence] : surface }}
    >
      {presence === 'idle' ? (
        <View className="absolute rounded-full" style={{ top: -size * 0.15, left: -size * 0.15, width: size * 0.65, height: size * 0.65, backgroundColor: surface }} />
      ) : null}
      {presence === 'dnd' ? <View className="rounded-full" style={{ width: size * 0.6, height: size * 0.22, backgroundColor: surface }} /> : null}
    </View>
  )
}
