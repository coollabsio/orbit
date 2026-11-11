import { toGitPatch } from '@orbit/api/github'
import { openURL } from 'expo-linking'
import { router, useLocalSearchParams } from 'expo-router'
import { useMemo } from 'react'
import { View } from 'react-native'

import { PullDiffView } from '../../../../../../../components/pulls/PullDiffView'
import { usePull, usePullFiles, usePullThreads } from '../../../../../../../hooks/pulls'
import { useTheme } from '../../../../../../../theme'
import { Button, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../../../../../../ui'

/** One file's diff (unified: a phone is too narrow for split). Lines and threads open native sheets (plan 3). */
export default function PullFileScreen() {
  const params = useLocalSearchParams<{ owner: string; repo: string; number: string; path: string }>()
  const ref = useMemo(() => ({ owner: params.owner, repo: params.repo, number: Number(params.number) }), [params.owner, params.repo, params.number])
  const { colors } = useTheme()
  const pull = usePull(ref)
  const files = usePullFiles(ref)
  const threads = usePullThreads(ref)
  const file = files.data?.find((item) => item.path === params.path)
  const patch = file ? toGitPatch(file) : null
  const name = params.path.slice(params.path.lastIndexOf('/') + 1)
  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back" color={colors.mutedForeground} onPress={() => router.back()} />
        <View className="min-w-0 flex-1">
          <Text numberOfLines={1} className="font-noto-semibold text-[13px]">
            {name}
          </Text>
          {file ? (
            <Text className="text-[11px] text-muted-foreground">
              +{file.additions} −{file.deletions}
            </Text>
          ) : null}
        </View>
      </PaneHeader>
      {!files.data || !pull.data ? (
        <LoadingView />
      ) : !file ? (
        <Text className="p-6 text-center text-[14px] text-muted-foreground">This file is not in the pull request any more.</Text>
      ) : !patch ? (
        <View className="items-center gap-3 p-6">
          <Text className="text-center text-[14px] text-muted-foreground">{file.status === 'renamed' && file.additions + file.deletions === 0 ? 'Renamed, no changes.' : 'Too large to show here.'}</Text>
          <Button label="Open on GitHub" variant="outline" onPress={() => void openURL(`${pull.data!.html_url}/files`)} />
        </View>
      ) : (
        <PullDiffView path={file.path} patch={patch} threads={threads.data?.threads ?? []} onThreadPress={() => {}} onLinesSelected={() => {}} />
      )}
    </SafeAreaView>
  )
}
