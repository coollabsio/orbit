import { router } from 'expo-router'
import { useState } from 'react'
import { RefreshControl, ScrollView, View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { pageTitle } from '../../../data/docs'
import type { TrashedPage } from '../../../data/docs-api'
import { relativeAgo } from '../../../data/format'
import { usePageTrash, useTrashWrites } from '../../../hooks/docs'
import { useTeamspaces } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Button, EmptyState, ErrorView, Icon, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../../ui'

/**
 * The web's docs Trash: pages trashed directly (their sub-pages come back with them), newest first, with Restore and
 * Delete forever for the pages the server marks `can_purge`.
 */
export default function DocsTrashScreen() {
  const { colors } = useTheme()
  const trash = usePageTrash()
  const teamspaces = useTeamspaces()
  const { restore, purge, empty } = useTrashWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const busy = restore.isPending || purge.isPending || empty.isPending
  const failure = restore.error ?? purge.error ?? empty.error
  const purgeable = (trash.data ?? []).filter((page) => page.can_purge).length
  const plural = (count: number) => `${count} page${count === 1 ? '' : 's'}`
  const requestEmpty = () => {
    const kept = (trash.data?.length ?? 0) - purgeable
    setConfirm({
      title: 'Empty trash?',
      description:
        `${plural(purgeable)} and the sub-pages trashed with them will be deleted forever. This can’t be undone.` +
        (kept > 0 ? ` ${plural(kept)} in teamspaces stay: only workspace owners and admins can delete those.` : ''),
      confirmLabel: 'Empty trash',
      danger: true,
      onConfirm: () => empty.mutate(),
    })
  }
  const spaceOf = (page: TrashedPage) => (page.private || !page.teamspace_id ? 'Private' : (teamspaces.data?.find((item) => item.id === page.teamspace_id)?.name ?? 'Teamspace'))

  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="px-3">
        <IconButton icon="back" label="Back to docs" color={alpha(colors.mutedForeground, 0.7)} onPress={() => (router.canGoBack() ? router.back() : router.navigate('/docs'))} />
        <Icon name="trash" size={15} color={colors.mutedForeground} />
        <Text className="font-noto-semibold text-[13px] flex-1">Trash</Text>
        {purgeable > 0 ? <Button label="Empty trash" size="sm" variant="ghost" disabled={busy} onPress={requestEmpty} /> : null}
      </PaneHeader>
      {failure ? <Text className="text-[13px] mx-4 mb-2 text-destructive">{errorMessage(failure)}</Text> : null}
      {!trash.data ? (
        trash.isError ? (
          <ErrorView error={trash.error} onRetry={() => void trash.refetch()} />
        ) : (
          <LoadingView />
        )
      ) : trash.data.length === 0 ? (
        <EmptyState icon="trash" title="Trash is empty" description="Pages you move to the trash show here." />
      ) : (
        <ScrollView contentContainerClassName="pb-6" refreshControl={<RefreshControl refreshing={trash.isRefetching} onRefresh={() => void trash.refetch()} tintColor={colors.mutedForeground} />}>
          {trash.data.map((page, index) => (
            <View key={page.id} className={`flex-row items-center gap-2.5 px-4 py-2.5 border-border ${index === 0 ? 'border-t-0' : 'border-t'}`}>
              <View className="w-4.5 items-center">{page.icon ? <Text className="text-[15px]">{page.icon}</Text> : <Icon name="docs" size={15} color={colors.mutedForeground} />}</View>
              <View className="flex-1 min-w-0">
                <Text numberOfLines={1} className="font-noto-medium text-[13px]">
                  {pageTitle(page)}
                </Text>
                <Text numberOfLines={1} className="text-[12px] text-muted-foreground">
                  {`${spaceOf(page)} · deleted ${relativeAgo(page.deleted_at)}`}
                </Text>
              </View>
              <Button label="Restore" icon="rotateLeft" size="sm" variant="outline" disabled={busy} onPress={() => restore.mutate(page)} />
              {page.can_purge ? (
                <IconButton
                  icon="trash"
                  label={`Delete ${pageTitle(page)} forever`}
                  color={colors.destructive}
                  disabled={busy}
                  onPress={() =>
                    setConfirm({
                      title: `Delete “${pageTitle(page)}” forever?`,
                      description: 'The page, the sub-pages trashed with it and their files are deleted permanently. This can’t be undone.',
                      confirmLabel: 'Delete forever',
                      danger: true,
                      onConfirm: () => purge.mutate(page),
                    })
                  }
                />
              ) : null}
            </View>
          ))}
        </ScrollView>
      )}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SafeAreaView>
  )
}
