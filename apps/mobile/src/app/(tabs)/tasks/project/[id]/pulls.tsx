import { pullPath } from '@orbit/api/github'
import type { GithubPullSummary } from '@orbit/api/generated/types.gen'
import { router, useLocalSearchParams } from 'expo-router'
import { useState } from 'react'
import { FlatList, Pressable, ScrollView, View } from 'react-native'

import { ConnectGithubPrompt } from '../../../../../components/github/ConnectGithubPrompt'
import { GithubAvatar, PullStateBadge } from '../../../../../components/pulls/PullBits'
import { relativeAgo } from '../../../../../data/format'
import { githubPromptStatus, PULL_FILTERS, type PullFilter } from '../../../../../data/pulls-api'
import { usePullList } from '../../../../../hooks/pulls'
import { useProjects } from '../../../../../queries'
import { useTheme } from '../../../../../theme'
import { ErrorView, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../../../ui'

/** The web's project pull request list: live from GitHub with the member's own account. */
export default function ProjectPullsScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { colors } = useTheme()
  const project = useProjects().data?.find((item) => item.id === id)
  const [filter, setFilter] = useState<PullFilter>('open')
  const list = usePullList(id, filter)
  const items = list.data?.pages.flatMap((page) => page.items) ?? []
  const prompt = githubPromptStatus(list.error)
  const shown = PULL_FILTERS.find((item) => item.value === filter) ?? PULL_FILTERS[0]!

  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back" color={colors.mutedForeground} onPress={() => router.back()} />
        <Text numberOfLines={1} className="flex-1 font-noto-semibold text-[14px]">
          {project ? `${project.name} · Pull requests` : 'Pull requests'}
        </Text>
      </PaneHeader>
      <View>
        <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerClassName="gap-1.5 px-3 py-2">
          {PULL_FILTERS.map((item) => (
            <Pressable
              key={item.value}
              accessibilityRole="tab"
              accessibilityState={{ selected: item.value === filter }}
              onPress={() => setFilter(item.value)}
              className={`h-8 justify-center rounded-full border px-3 ${item.value === filter ? 'border-foreground bg-foreground' : 'border-border'}`}
            >
              <Text className={`text-[13px] ${item.value === filter ? 'text-background' : 'text-foreground'}`}>{item.label}</Text>
            </Pressable>
          ))}
        </ScrollView>
      </View>
      {prompt ? (
        <ConnectGithubPrompt status={prompt} />
      ) : list.isError ? (
        <ErrorView error={list.error} onRetry={() => void list.refetch()} />
      ) : list.isPending ? (
        <LoadingView />
      ) : (
        <FlatList
          data={items}
          keyExtractor={(item) => `${item.repository}#${item.number}`}
          renderItem={({ item }) => <PullRow item={item} />}
          refreshControl={<PullRefresh onRefresh={() => list.refetch()} />}
          onEndReached={() => {
            if (list.hasNextPage && !list.isFetchingNextPage) void list.fetchNextPage()
          }}
          ListEmptyComponent={<Text className="p-6 text-center text-[14px] text-muted-foreground">{shown.empty}</Text>}
          contentContainerClassName="pb-8"
        />
      )}
    </SafeAreaView>
  )
}

function PullRow({ item }: { item: GithubPullSummary }) {
  const [owner = '', repo = ''] = item.repository.split('/')
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={`${item.title} #${item.number}`} onPress={() => router.push(pullPath({ owner, repo, number: item.number }) as never)} className="gap-1.5 border-b border-border px-4 py-3 active:bg-muted">
      <View className="flex-row items-center gap-2">
        <PullStateBadge state={item.state} />
        <Text numberOfLines={2} className="flex-1 font-noto-medium text-[14px] leading-[19px]">
          {item.title}
        </Text>
      </View>
      <View className="flex-row items-center gap-1.5">
        <GithubAvatar actor={item.author} size={16} />
        <Text className="text-[12px] text-muted-foreground">
          #{item.number} · {item.author.login} · {relativeAgo(new Date(item.updated_at).toISOString())}
          {item.comments > 0 ? ` · ${item.comments} comments` : ''}
        </Text>
      </View>
    </Pressable>
  )
}
