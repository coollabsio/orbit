import { useQuery } from '@tanstack/react-query'
import { type Href, router } from 'expo-router'
import type { ReactNode } from 'react'
import { Pressable, RefreshControl, ScrollView, View } from 'react-native'

import { MenuButton } from '../../components/Drawer'
import { relativeTime } from '../../data/format'
import { notificationCopy, notificationTarget } from '../../data/inbox'
import { keys } from '../../data/keys'
import * as api from '../../data/orbit'
import { dueLabel, identifier } from '../../data/tasks'
import { useOrbit } from '../../orbit-context'
import { usePages, usePeople, useProjects, useStatuses, useUnreadNotifications } from '../../queries'
import { alpha, useTheme } from '../../theme'
import { HeaderTitle, Icon, IconButton, PaneHeader, SafeAreaView, StatusIcon, Text } from '../../ui'

/** Most rows a card shows: Home is a short brief, the full lists live in Tasks, Activity and Docs. */
const CARD_LIMIT = 7

function greetingFor(hour: number): string {
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

export default function HomeScreen() {
  const { client, user, workspaceId } = useOrbit()
  const { colors } = useTheme()
  const projects = useProjects()
  const statuses = useStatuses()
  const people = usePeople()
  const pages = usePages()
  const unread = useUnreadNotifications()
  // the web's cards: the first rows of the "My week" and "Overdue" task pages
  const week = useQuery({
    queryKey: keys.taskView(workspaceId, 'home-week'),
    queryFn: () => api.taskPage(client, workspaceId, { view: 'my_week', sort: 'due_date', order: 'asc', limit: CARD_LIMIT }),
  })
  const overdue = useQuery({
    queryKey: keys.taskView(workspaceId, 'home-overdue'),
    queryFn: () => api.taskPage(client, workspaceId, { view: 'overdue', assignee_id: user.id, sort: 'due_date', order: 'asc', limit: CARD_LIMIT }),
  })
  const now = new Date()
  const firstName = user.display_name.split(' ')[0] ?? user.display_name

  const queries = [week, overdue, unread, pages, projects, statuses]
  const refresh = () => Promise.all(queries.map((query) => query.refetch()))
  const weekTasks = week.data?.items ?? []
  const weekCount = `${weekTasks.length}${week.data?.more ? '+' : ''}`
  const unreadItems = unread.data ?? []
  const recentDocs = [...(pages.data ?? [])].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, CARD_LIMIT)
  const statusOf = (task: api.TaskRecord) => statuses.data?.find((status) => status.id === task.status_id)
  const taskRow = (task: api.TaskRecord, index: number, list: readonly api.TaskRecord[]) => {
    const due = task.due_at ? dueLabel(task.due_at, now) : null
    return (
      <CardRow key={task.id} href={`/tasks/${task.id}`} last={index === list.length - 1}>
        <StatusIcon status={statusOf(task)} size={14} />
        <Meta>{identifier(task, projects.data ?? [])}</Meta>
        <Text numberOfLines={1} className="text-[13px] flex-1">
          {task.title}
        </Text>
        {due ? <Meta destructive={due === 'Overdue'}>{due}</Meta> : null}
      </CardRow>
    )
  }

  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader>
        <MenuButton />
        <HeaderTitle size={14}>Home</HeaderTitle>
        <View className="flex-1" />
        <IconButton icon="search" label="Search" onPress={() => router.navigate('/tasks?search=1')} />
      </PaneHeader>
      <ScrollView
        contentContainerClassName="px-4 pt-8 pb-6 gap-6"
        refreshControl={<RefreshControl refreshing={queries.some((query) => query.isRefetching)} onRefresh={() => void refresh()} tintColor={colors.mutedForeground} />}
      >
        <View>
          <Text className="font-noto-semibold text-[24px] leading-[28px] tracking-[-0.6px] mb-1.5">
            {greetingFor(now.getHours())}, {firstName}
          </Text>
          <Text className="text-[13px] text-muted-foreground">
            <Text onPress={() => router.navigate('/tasks?view=my_week')} className="text-[13px] text-muted-foreground">
              {weekCount} {weekCount === '1' ? 'task' : 'tasks'} due this week
            </Text>
            {' · '}
            <Text onPress={() => router.navigate('/activity')} className="text-[13px] text-muted-foreground">
              {unreadItems.length} unread {unreadItems.length === 1 ? 'notification' : 'notifications'}
            </Text>
          </Text>
        </View>

        <View className="gap-4">
          {overdue.data && overdue.data.items.length > 0 ? (
            <HomeCard title="Overdue" href="/tasks?view=overdue">
              {overdue.data.items.map(taskRow)}
            </HomeCard>
          ) : null}

          <HomeCard title="Due this week" href="/tasks?view=my_week">
            {weekTasks.length === 0 ? <CardEmpty>Nothing due this week.</CardEmpty> : weekTasks.map(taskRow)}
          </HomeCard>

          <HomeCard title="Unread in Inbox" href="/activity">
            {unreadItems.length === 0 ? (
              <CardEmpty>You are all caught up.</CardEmpty>
            ) : (
              unreadItems.slice(0, CARD_LIMIT).map((notification, index, list) => {
                const page = pages.data?.find((item) => item.id === notification.page_id)
                const actor = people.find((person) => person.id === notification.actor_user_id)
                return (
                  <CardRow key={notification.id} href={(notificationTarget(notification) ?? '/activity') as Href} last={index === list.length - 1}>
                    <Icon name="directInbox" size={16} color={alpha(colors.mutedForeground, 0.7)} />
                    <Text numberOfLines={1} className="text-[13px] flex-1">
                      {notificationCopy(notification, page?.title, actor?.name).title}
                    </Text>
                    <Meta>{relativeTime(notification.created_at, now)}</Meta>
                  </CardRow>
                )
              })
            )}
          </HomeCard>

          <HomeCard title="Recently updated docs" href="/docs">
            {recentDocs.length === 0 ? (
              <CardEmpty>No docs yet.</CardEmpty>
            ) : (
              recentDocs.map((page, index) => (
                <CardRow key={page.id} href={`/docs/${page.id}`} last={index === recentDocs.length - 1}>
                  {page.icon ? <Text className="text-[16px]">{page.icon}</Text> : <Icon name="docs" size={16} color={alpha(colors.mutedForeground, 0.7)} />}
                  <Text numberOfLines={1} className="text-[13px] flex-1">
                    {page.title.trim() || 'Untitled'}
                  </Text>
                  <Meta>{relativeTime(page.updated_at, now)}</Meta>
                </CardRow>
              ))
            )}
          </HomeCard>
        </View>
      </ScrollView>
    </SafeAreaView>
  )
}

/** A card with a muted title and "View all" on the card surface, the rows on an inset background panel. */
function HomeCard({ title, href, children }: { title: string; href: Href; children: ReactNode }) {
  return (
    <View className="rounded-lg bg-card shadow-[0_0_0_1px_var(--color-border)]">
      <View className="min-h-12 flex-row items-center py-2 pr-2 pl-4">
        <Text className="font-noto-medium text-[14px] flex-1 text-muted-foreground">{title}</Text>
        <Pressable
          accessibilityRole="link"
          accessibilityLabel={`View all: ${title}`}
          onPress={() => router.navigate(href)}
          className="h-8 px-2.5 rounded-lg justify-center active:bg-muted"
        >
          <Text className="font-noto-medium text-[14px] text-muted-foreground">View all</Text>
        </Pressable>
      </View>
      <View className="rounded-lg overflow-hidden bg-background shadow-[0_0_0_1px_var(--color-muted)]">{children}</View>
    </View>
  )
}

function CardRow({ href, last, children }: { href: Href; last: boolean; children: ReactNode }) {
  return (
    <Pressable
      accessibilityRole="link"
      onPress={() => router.navigate(href)}
      className={`min-h-10 flex-row items-center gap-2.5 px-3 py-1.5 border-border active:bg-foreground/2 ${last ? 'border-b-0' : 'border-b'}`}
    >
      {children}
    </Pressable>
  )
}

function CardEmpty({ children }: { children: ReactNode }) {
  return <Text className="text-[13px] p-4 text-muted-foreground/70">{children}</Text>
}

function Meta({ children, destructive = false }: { children: ReactNode; destructive?: boolean }) {
  return <Text className={`text-[12px] leading-[16px] ${destructive ? 'text-destructive' : 'text-muted-foreground/70'}`}>{children}</Text>
}
