import { router } from 'expo-router'
import { useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { filterTrash, pageTitle } from '../../../../data/docs'
import type { TrashedPage } from '../../../../data/docs-api'
import { usePageTrash, useTrashWrites } from '../../../../hooks/docs'
import { useTeamspaces } from '../../../../queries'
import { errorMessage } from '../../../../session-context'
import { alpha, useTheme } from '../../../../theme'
import { Button, EmptyState, ErrorView, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../../ui'
import { TextInput } from '../../../../ui/scaled'

/**
 * The web's docs Trash: pages trashed directly (their sub-pages come back with them), newest first, with Restore and
 * Delete forever for the pages the server marks `can_purge`. A row opens the page's read-only preview.
 */
export default function DocsTrashScreen() {
  const { colors } = useTheme()
  const trash = usePageTrash()
  const teamspaces = useTeamspaces()
  const { restore, purge, empty } = useTrashWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [query, setQuery] = useState('')
  const shown = filterTrash(trash.data ?? [], query)
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
  const isPrivate = (page: TrashedPage) => page.private || !page.teamspace_id
  const spaceOf = (page: TrashedPage) => (isPrivate(page) ? 'Private' : (teamspaces.data?.find((item) => item.id === page.teamspace_id)?.name ?? 'Teamspace'))

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back to docs" color={alpha(colors.mutedForeground, 0.7)} onPress={() => (router.canGoBack() ? router.back() : router.navigate('/docs'))} />
        <Text numberOfLines={1} className="font-noto-semibold text-[13px] flex-1 min-w-0">
          Trash
        </Text>
        {purgeable > 0 ? (
          // the web's ghost `sm` button with `text-destructive`
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Empty trash"
            disabled={busy}
            onPress={requestEmpty}
            className={`h-7 flex-row items-center rounded-md px-2.5 active:bg-muted dark:active:bg-muted/50 ${busy ? 'opacity-50' : ''}`}
          >
            <Text className="font-noto-medium text-[12.8px] leading-[18px] text-destructive">Empty trash</Text>
          </Pressable>
        ) : null}
      </PaneHeader>
      {trash.data?.length ? (
        // the web's search field in the header, on its own row at phone width
        <View className="mx-3 mb-2 h-8 flex-row items-center gap-1.5 px-2.5 rounded-lg border border-input dark:bg-input/30">
          <Icon name="search" size={14} color={colors.mutedForeground} />
          <TextInput
            accessibilityLabel="Search the trash"
            placeholder="Search pages…"
            placeholderTextColor={colors.mutedForeground}
            value={query}
            onChangeText={setQuery}
            autoCapitalize="none"
            autoCorrect={false}
            returnKeyType="search"
            className="font-noto text-[16px] leading-[1.45] flex-1 min-w-0 py-0 text-foreground"
          />
        </View>
      ) : null}
      {failure ? <Text className="text-[13px] mx-4 mb-2 text-destructive">{errorMessage(failure)}</Text> : null}
      {!trash.data ? (
        trash.isError ? (
          <ErrorView error={trash.error} onRetry={() => void trash.refetch()} />
        ) : (
          <LoadingView />
        )
      ) : trash.data.length === 0 ? (
        <EmptyState icon="trash" title="Trash is empty" description="Pages you move to the trash show here." />
      ) : shown.length === 0 ? (
        <EmptyState icon="search" title="No results" description="No page in the trash has this title." />
      ) : (
        <ScrollView contentContainerClassName="pb-6" keyboardShouldPersistTaps="handled" refreshControl={<PullRefresh onRefresh={() => trash.refetch()} />}>
          {shown.map((page) => (
            <View key={page.id} className="min-h-10 flex-row items-center gap-2.5 border-b border-border px-3 py-1.5">
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`Preview ${pageTitle(page)}`}
                onPress={() => router.push(`/docs/trash/${page.id}`)}
                className="flex-1 min-w-0 flex-row items-center gap-2.5 self-stretch rounded-sm active:bg-muted dark:active:bg-muted/50"
              >
                <View className="w-[18px] items-center">{page.icon ? <Text className="text-[15px]">{page.icon}</Text> : <Icon name="docs" size={15} color={alpha(colors.mutedForeground, 0.7)} />}</View>
                <Text numberOfLines={1} className="flex-1 min-w-0 text-[13px]">
                  {pageTitle(page)}
                </Text>
                <View className="max-w-40 min-w-0 shrink flex-row items-center gap-1">
                  {isPrivate(page) ? <Icon name="lock" size={12} color={alpha(colors.mutedForeground, 0.7)} /> : null}
                  <Text numberOfLines={1} className="shrink text-[12px] text-muted-foreground/70">
                    {spaceOf(page)}
                  </Text>
                </View>
              </Pressable>
              <Button label="Restore" icon="refresh" size="sm" variant="outline" disabled={busy} onPress={() => restore.mutate(page)} />
              {page.can_purge ? (
                <IconButton
                  icon="trash"
                  iconSize={14}
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
