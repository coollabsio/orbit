import type { GithubActor } from '@orbit/api/generated/types.gen'
import { Image, View } from 'react-native'

import { Text } from '../../ui'

const STATE: Record<string, { label: string; className: string; text: string }> = {
  open: { label: 'Open', className: 'bg-emerald-500/15', text: 'text-emerald-700 dark:text-emerald-400' },
  draft: { label: 'Draft', className: 'bg-muted', text: 'text-muted-foreground' },
  merged: { label: 'Merged', className: 'bg-violet-500/15', text: 'text-violet-700 dark:text-violet-400' },
  closed: { label: 'Closed', className: 'bg-red-500/15', text: 'text-red-700 dark:text-red-400' },
}

/** The web's `PullStateBadge`. */
export function PullStateBadge({ state }: { state: string }) {
  const shown = STATE[state] ?? STATE.open!
  return (
    <View className={`h-5 shrink-0 items-center justify-center rounded-full px-2 ${shown.className}`}>
      <Text className={`font-noto-medium text-[11px] leading-[14px] ${shown.text}`}>{shown.label}</Text>
    </View>
  )
}

/** A GitHub account's picture. */
export function GithubAvatar({ actor, size = 20 }: { actor: GithubActor; size?: number }) {
  return actor.avatar_url ? <Image source={{ uri: actor.avatar_url }} style={{ width: size, height: size, borderRadius: size / 2 }} accessibilityIgnoresInvertColors /> : <View className="rounded-full bg-muted" style={{ width: size, height: size }} />
}
