import { ApiProblem } from '@orbit/api/problem'
import { type Href, router, useLocalSearchParams } from 'expo-router'
import { useCallback, useState } from 'react'
import { Image, Pressable, ScrollView, View } from 'react-native'

import { EditorView } from '../../../../components/docs/EditorView'
import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { absoluteUrl, daysUntilPurge, pageTitle } from '../../../../data/docs'
import type { PageSummary } from '../../../../data/orbit'
import { useTrashedPage, useTrashWrites } from '../../../../hooks/docs'
import { useOrbit } from '../../../../orbit-context'
import { useMembers } from '../../../../queries'
import { errorMessage } from '../../../../session-context'
import { alpha, useTheme } from '../../../../theme'
import { EmptyState, ErrorView, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../../ui'

const noop = () => {}

/** The sub-pages under `parentId` in tree order, each with its depth below the previewed page. */
function nested(pages: readonly PageSummary[], parentId: string, depth = 0): { page: PageSummary; depth: number }[] {
  return pages
    .filter((page) => page.parent_id === parentId)
    .sort((a, b) => a.position - b.position || a.id.localeCompare(b.id))
    .flatMap((page) => [{ page, depth }, ...nested(pages, page.id, depth + 1)])
}

/** The web's banner button: an outlined white button on the red banner. */
function BannerButton({ label, disabled, onPress }: { label: string; disabled: boolean; onPress: () => void }) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      disabled={disabled}
      onPress={onPress}
      className={`h-7 flex-row items-center rounded-md border border-white/70 px-2.5 active:bg-white/15 ${disabled ? 'opacity-50' : ''}`}
    >
      <Text className="font-noto-medium text-[12.8px] leading-[18px] text-white">{label}</Text>
    </Pressable>
  )
}

/**
 * The web's trashed page preview: a banner with who trashed it, when the trash deletes it, Restore page and (for
 * `can_purge`) Delete forever, then the page read-only (no co-editing socket) and the sub-pages trashed with it.
 */
export default function TrashedPageScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { session } = useOrbit()
  const { colors } = useTheme()
  const detail = useTrashedPage(id)
  const members = useMembers()
  const { restore, purge } = useTrashWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const busy = restore.isPending || purge.isPending
  const failure = restore.error ?? purge.error
  const subPages = detail.data?.sub_pages
  const pageHref = useCallback((target: string): Href => (subPages?.some((page) => page.id === target) ? `/docs/trash/${target}` : `/docs/${target}`), [subPages])
  const leave = () => (router.canGoBack() ? router.back() : router.replace('/docs/trash'))

  const header = (
    <PaneHeader className="min-h-12 px-3">
      <IconButton icon="back" label="Back to the trash" color={alpha(colors.mutedForeground, 0.7)} onPress={leave} />
      <Text numberOfLines={1} className="font-noto-semibold text-[13px] flex-1 min-w-0">
        Trash
      </Text>
    </PaneHeader>
  )

  if (!detail.data) {
    const missing = detail.error instanceof ApiProblem && detail.error.status === 404
    return (
      <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
        {header}
        {missing ? (
          <EmptyState icon="trash" title="Page not in the trash" description="It was restored or deleted forever, or its 30 days in the trash are over." />
        ) : detail.isError ? (
          <ErrorView error={detail.error} onRetry={() => void detail.refetch()} />
        ) : (
          <LoadingView />
        )}
      </SafeAreaView>
    )
  }

  const { page, can_purge: canPurge, deleted_by: deletedBy } = detail.data
  const who = members.data?.find((member) => member.user_id === deletedBy)?.display_name ?? 'Someone'
  const deletedAt = page.deleted_at ?? page.updated_at
  const days = daysUntilPurge(deletedAt)
  const when = new Date(deletedAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  const children = nested(detail.data.sub_pages, page.id)

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      {header}
      <View accessibilityRole="alert" className="gap-2 bg-destructive px-3 py-2">
        <Text className="text-[13px] leading-[18px] text-white">
          {who} moved this page to Trash {when}. It will be permanently deleted {days === 0 ? 'today' : `in ${days} day${days === 1 ? '' : 's'}`}.
        </Text>
        <View className="flex-row gap-2">
          <BannerButton
            label="Restore page"
            disabled={busy}
            onPress={() => restore.mutate(page, { onSuccess: (restored) => router.replace(`/docs/${restored.id}`) })}
          />
          {canPurge ? (
            <BannerButton
              label="Delete forever"
              disabled={busy}
              onPress={() =>
                setConfirm({
                  title: `Delete “${pageTitle(page)}” forever?`,
                  description: 'The page, the sub-pages trashed with it and their files are deleted permanently. This can’t be undone.',
                  confirmLabel: 'Delete forever',
                  danger: true,
                  onConfirm: () => purge.mutate(page, { onSuccess: leave }),
                })
              }
            />
          ) : null}
        </View>
      </View>
      {failure ? <Text className="text-[13px] mx-4 my-2 text-destructive">{errorMessage(failure)}</Text> : null}
      <ScrollView contentContainerClassName="pb-8" refreshControl={<PullRefresh onRefresh={() => detail.refetch()} />}>
        {page.cover_url ? (
          <Image
            accessibilityLabel="Cover"
            source={{ uri: absoluteUrl(session.server, page.cover_url), headers: page.cover_url.startsWith('/') ? { Authorization: `Bearer ${session.token}` } : undefined }}
            className="h-35 w-full bg-muted"
            resizeMode="cover"
          />
        ) : null}
        <View className="px-3 pt-6">
          {page.icon ? <Text className="text-[40px] leading-[48px] mb-2 self-start">{page.icon}</Text> : null}
          <Text className="font-noto-bold text-[16px] leading-[20px] mb-[17px] text-foreground">{pageTitle(page)}</Text>
        </View>
        <EditorView
          key={page.id}
          pageId={page.id}
          epoch={page.collab_epoch}
          generation={0}
          editable={false}
          content={page.content}
          pageHref={pageHref}
          extraPages={detail.data.sub_pages}
          onProblem={noop}
          onProbe={noop}
          onFormat={noop}
        />
        {children.length > 0 ? (
          <View accessibilityLabel="Sub-pages in the trash" className="mx-3 mt-6 border-t border-border pt-3">
            <Text className="mb-1 font-noto-medium text-[12px] text-muted-foreground">Sub-pages trashed with it</Text>
            {children.map(({ page: child, depth }) => (
              <Pressable
                key={child.id}
                accessibilityRole="link"
                onPress={() => router.push(`/docs/trash/${child.id}`)}
                className="min-h-9 flex-row items-center gap-2 rounded-md active:bg-muted dark:active:bg-muted/50"
                style={{ paddingLeft: 6 + depth * 18 }}
              >
                <View className="w-[18px] items-center">{child.icon ? <Text className="text-[15px]">{child.icon}</Text> : <Icon name="docs" size={15} color={alpha(colors.mutedForeground, 0.7)} />}</View>
                <Text numberOfLines={1} className="flex-1 min-w-0 text-[13px]">
                  {pageTitle(child)}
                </Text>
              </Pressable>
            ))}
          </View>
        ) : null}
      </ScrollView>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SafeAreaView>
  )
}
