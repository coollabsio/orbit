import { useMutation, useQueryClient } from '@tanstack/react-query'
import { type Href, router } from 'expo-router'
import { useState } from 'react'
import { Alert, FlatList, Pressable, View } from 'react-native'

import { relativeTime } from '../../data/format'
import { isMention, notificationCopy, notificationTarget } from '../../data/inbox'
import * as api from '../../data/orbit'
import { useOrbit } from '../../orbit-context'
import { useNotifications, usePages, usePeople } from '../../queries'
import { errorMessage } from '../../session-context'
import { Button, CountBadge, EmptyState, ErrorView, HeaderTitle, Icon, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../ui'
import { useTheme } from '../../theme'

type Tab = 'all' | 'unread' | 'mentions'
const TABS: { value: Tab; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'unread', label: 'Unread' },
  { value: 'mentions', label: 'Mentions' },
]

/** The web's Activity page: underline tabs and 56px rows. */
export default function ActivityScreen() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const [tab, setTab] = useState<Tab>('all')
  const notifications = useNotifications()
  const people = usePeople()
  const { colors } = useTheme()
  const pages = usePages()
  // both the list and the unread list live under this prefix
  const refreshInbox = () => queryClient.invalidateQueries({ queryKey: ['notifications', workspaceId] })

  const markRead = useMutation({
    mutationFn: (id: string) => api.markNotificationRead(client, workspaceId, id),
    onSettled: refreshInbox,
  })
  const markAll = useMutation({
    mutationFn: () => api.markAllNotificationsRead(client, workspaceId),
    onSettled: refreshInbox,
    onError: (error) => Alert.alert('Could not mark all read', errorMessage(error)),
  })
  const all = notifications.data ?? []
  const unreadCount = all.filter((item) => !item.read_at).length
  const items = all.filter((item) => (tab === 'unread' ? !item.read_at : tab === 'mentions' ? isMention(item) : true))

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <HeaderTitle>Activity</HeaderTitle>
        {unreadCount > 0 ? <CountBadge count={unreadCount} /> : null}
        <View className="flex-1" />
        <Button label={markAll.isPending ? 'Marking…' : 'Mark all read'} variant="ghost" disabled={unreadCount === 0 || markAll.isPending} onPress={() => markAll.mutate()} />
      </PaneHeader>
      <View accessibilityRole="tablist" className="min-h-10 flex-row items-end gap-1.5 px-3 border-b border-border">
        {TABS.map((item) => {
          const selected = item.value === tab
          return (
            <Pressable
              key={item.value}
              accessibilityRole="tab"
              accessibilityState={{ selected }}
              onPress={() => setTab(item.value)}
              className="h-[33px] justify-start pt-[5px]"
            >
              <Text className={`font-noto-medium text-[13px] px-2.5 ${selected ? 'text-foreground' : 'text-muted-foreground'}`}>{item.label}</Text>
              <View className={`absolute left-0 right-0 -bottom-px h-0.5 ${selected ? 'bg-foreground' : ''}`} />
            </Pressable>
          )
        })}
      </View>
      {!notifications.data ? (
        notifications.isError ? (
          <ErrorView error={notifications.error} onRetry={() => void notifications.refetch()} />
        ) : (
          <LoadingView />
        )
      ) : (
        <FlatList
          data={items}
          keyExtractor={(item) => item.id}
          contentContainerClassName={items.length === 0 ? 'grow' : undefined}
          refreshControl={<PullRefresh onRefresh={() => notifications.refetch()} />}
          ListEmptyComponent={<EmptyState icon="inbox" title="You're all caught up" description="Assignments and mentions appear here." />}
          renderItem={({ item }) => {
            const actor = people.find((person) => person.id === item.actor_user_id)
            const page = item.page_id ? pages.data?.find((entry) => entry.id === item.page_id) : undefined
            const copy = notificationCopy(item, page?.title, actor?.name)
            const unread = !item.read_at
            return (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`${unread ? 'Unread. ' : ''}${copy.title}`}
                onPress={() => {
                  if (unread) markRead.mutate(item.id)
                  const target = notificationTarget(item)
                  if (target) router.push(target as Href)
                }}
                className="min-h-14 flex-row items-center gap-2.5 px-3 py-1.5 border-b border-border active:bg-muted"
              >
                <View className="w-2 items-center">{unread ? <View accessibilityLabel="Unread" className="size-2 rounded-full bg-primary" /> : null}</View>
                <View className="size-7 rounded-full items-center justify-center bg-muted">
                  {/* an import notice is the user's own: the import glyph, not their initial */}
                  {item.import_id ? (
                    <Icon name="import" size={16} color={colors.mutedForeground} />
                  ) : (
                    <Text className="text-[14px] leading-[20px] text-muted-foreground">{(actor?.name ?? '?').charAt(0).toUpperCase()}</Text>
                  )}
                </View>
                <View className="flex-1 min-w-0 gap-0.5">
                  <Text numberOfLines={1} className={`text-[13px] leading-[18.5px] ${unread ? 'font-noto-semibold' : ''}`}>
                    {copy.title}
                  </Text>
                  <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground/70">
                    {actor && !item.import_id ? `${actor.name} · ${copy.body}` : copy.body}
                  </Text>
                </View>
                <Text className="text-[12px] leading-[16px] text-muted-foreground/70">{relativeTime(item.created_at)}</Text>
              </Pressable>
            )
          }}
        />
      )}
    </SafeAreaView>
  )
}
