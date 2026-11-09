import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Image, View } from 'react-native'

import { avatarColors, userColor } from '../data/color'
import { absoluteUrl } from '../data/docs'
import { keys } from '../data/keys'
import { members } from '../data/orbit'
import { useOrbitIfSignedIn } from '../orbit-context'
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
  /** The picture's URL, when the caller knows it. Else the signed-in user's own, or the workspace member's. */
  url?: string | null
}

/**
 * Where the person's picture is (`/api/v1/users/{id}/avatar?v=…`), as the web reads it: the signed-in user's from the
 * session, everyone else's from the workspace members (one shared query). `null` without a session or a picture.
 */
function usePictureUrl(id: string, url: string | null | undefined): { uri: string; headers: { Authorization: string } } | null {
  const orbit = useOrbitIfSignedIn()
  const list = useQuery({
    queryKey: keys.members(orbit?.workspaceId ?? ''),
    queryFn: () => members(orbit!.client, orbit!.workspaceId),
    enabled: !!orbit && url === undefined && id !== orbit.user.id,
  })
  if (!orbit) return null
  const path = url !== undefined ? url : id === orbit.user.id ? orbit.user.avatar_url : list.data?.find((member) => member.user_id === id)?.avatar_url
  // the endpoint needs the session: native Image sends no cookie, so the bearer token goes in a header (never the URL)
  return path ? { uri: absoluteUrl(orbit.session.server, path), headers: { Authorization: `Bearer ${orbit.session.token}` } } : null
}

/** The web `UserAvatar`: the person's picture, over the fallback of initials on a tint of their color on the muted surface. */
export function Avatar({ id, name, size = 24, presence, surface, url }: AvatarProps) {
  const { colors } = useTheme()
  const look = avatarColors(userColor(id), colors.muted, colors.foreground)
  const picture = usePictureUrl(id, url)
  const [failed, setFailed] = useState<string | null>(null)
  return (
    <View accessibilityLabel={name} style={{ width: size, height: size }}>
      <View className="items-center justify-center overflow-hidden rounded-full" style={{ width: size, height: size, backgroundColor: look.background }}>
        <Text className="font-noto-semibold" style={{ color: look.text, fontSize: Math.max(9, Math.round(size * 0.38)), lineHeight: size }}>
          {initialsOf(name)}
        </Text>
        {picture && failed !== picture.uri ? (
          <Image source={picture} onError={() => setFailed(picture.uri)} className="absolute inset-0" style={{ width: size, height: size }} />
        ) : null}
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
