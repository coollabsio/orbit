import type { PullRef } from '@orbit/api/github'
import { router } from 'expo-router'
import { Pressable, View } from 'react-native'

import { pullRoute } from '../../data/pull-route'
import { usePull } from '../../hooks/pulls'
import { Text } from '../../ui'
import { PullStateBadge } from './PullBits'

/** The web's chat card for a GitHub pull request link: only for a reader whose GitHub account can see it. */
export function GithubPullCard({ pullRef }: { pullRef: PullRef }) {
  const pull = usePull(pullRef)
  if (!pull.data) return null
  const data = pull.data
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={`${data.title} #${data.number}`} onPress={() => router.push(pullRoute(pullRef) as never)} className="mt-1.5 gap-1 rounded-lg border border-border px-3 py-2 active:bg-muted">
      <View className="flex-row items-center gap-2">
        <PullStateBadge state={data.state} />
        <Text numberOfLines={1} className="flex-1 font-noto-medium text-[13px]">
          {data.title}
        </Text>
      </View>
      <Text className="text-[12px] text-muted-foreground">
        {data.repository}#{data.number} · <Text className="text-[12px] text-emerald-700 dark:text-emerald-400">+{data.additions}</Text>{' '}
        <Text className="text-[12px] text-red-700 dark:text-red-400">−{data.deletions}</Text>
      </Text>
    </Pressable>
  )
}
