import { parseGithubPullUrl, pullPath } from '@orbit/api/github'
import { openURL } from 'expo-linking'
import { router } from 'expo-router'
import { Pressable, View } from 'react-native'

import { useTaskGithubLinks } from '../../hooks/pulls'
import { useTheme } from '../../theme'
import { Icon, Text } from '../../ui'

const STATE_LABEL: Record<string, string> = { paused: 'Paused PR', merged: 'Merged PR', closed: 'Closed PR' }

/** The web's GitHub section of a task: each linked pull request opens the viewer; the arrow opens it on GitHub. */
export function TaskPulls({ taskId }: { taskId: string }) {
  const { colors } = useTheme()
  const links = (useTaskGithubLinks(taskId).data ?? []).filter((link) => link.kind === 'pull_request' && !link.source)
  if (links.length === 0) return null
  return (
    <View>
      <Text className="mb-2 font-noto-semibold text-[14px] leading-[20px]">GitHub</Text>
      <View className="overflow-hidden rounded-lg border border-border">
        {links.map((link, index) => {
          const ref = parseGithubPullUrl(link.url)
          return (
            <View key={link.url} className={`min-h-11 flex-row items-center gap-2 border-border pl-3 ${index === 0 ? '' : 'border-t'}`}>
              <Pressable accessibilityRole="link" accessibilityLabel={link.title} onPress={() => (ref ? router.push(pullPath(ref) as never) : void openURL(link.url))} className="min-w-0 flex-1 flex-row items-center gap-2 py-2.5 active:opacity-60">
                <Icon name="link2" size={14} color={colors.mutedForeground} />
                <Text numberOfLines={1} className="flex-1 text-[13px]">
                  {link.title}
                </Text>
                <Text className="text-[12px] text-muted-foreground">{STATE_LABEL[link.state] ?? 'Open PR'}</Text>
              </Pressable>
              {ref ? (
                <Pressable accessibilityRole="link" accessibilityLabel="Open on GitHub" onPress={() => void openURL(link.url)} hitSlop={6} className="size-10 items-center justify-center active:opacity-60">
                  <Icon name="arrowUpRightSquare" size={14} color={colors.mutedForeground} />
                </Pressable>
              ) : (
                <View className="w-3" />
              )}
            </View>
          )
        })}
      </View>
    </View>
  )
}
