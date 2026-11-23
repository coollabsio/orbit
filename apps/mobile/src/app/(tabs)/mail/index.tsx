import type { MailLabel } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { router, useLocalSearchParams } from 'expo-router'
import { type ReactElement, useMemo, useState } from 'react'
import { Alert, FlatList, Pressable, ScrollView, View } from 'react-native'

import { ActionSheet, type SheetAction } from '../../../components/settings/Dialog'
import { ThreadRow } from '../../../components/mail/ThreadRow'
import { UndoSendBar } from '../../../components/mail/UndoSendBar'
import { fullDate, relativeTime, timeOfDay } from '../../../data/format'
import { mailKeys } from '../../../data/keys'
import { groupThreads, type ListEntry, parseTab, shownTabs, TAB_INFO, threadActions, type MailTab, type ThreadChange } from '../../../data/mail'
import * as mail from '../../../data/mail-api'
import type { MailDraft, MailOutboundItem, MailThreadSummary } from '../../../data/mail-api'
import { htmlToText, recipientSummary } from '../../../data/mail-compose'
import { outboxStatus } from '../../../data/mail-status'
import type { SwipeStep } from '../../../data/mail-swipe'
import { confirmAction } from '../../../hooks/chat'
import { refreshSendViews, useDrafts, useMailCounts, useMailMe, useMailSearch, useOutbound, useSendPolicy, useStartDraft, useThreads, useUpdateThread } from '../../../hooks/mail'
import { useMailSwipes } from '../../../hooks/mail-swipe'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Button, EmptyState, ErrorView, HeaderTitle, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Spinner, Text } from '../../../ui'
import { TextInput } from '../../../ui/scaled'

/** The web's mail page at phone size: the search box, the folder tabs, and the list of the open tab. */
export default function MailScreen() {
  const params = useLocalSearchParams<{ tab?: string }>()
  const tab = parseTab(params.tab)
  const { colors, scheme } = useTheme()
  const me = useMailMe()
  const counts = useMailCounts()
  const [unreadOnly, setUnreadOnly] = useState(false)
  const [typed, setTyped] = useState('')
  const [query, setQuery] = useState('')
  const [menu, setMenu] = useState<{ title: string; actions: SheetAction[] } | null>(null)
  const update = useUpdateThread()
  const start = useStartDraft()
  const swipes = useMailSwipes()
  const source = TAB_INFO[tab].source
  const searching = query.trim() !== ''
  const threadFilter = source.kind === 'threads' ? (unreadOnly ? { ...source.filter, unread: true } : source.filter) : {}
  const list = useThreads(threadFilter, !searching && source.kind === 'threads')
  const search = useMailSearch(query)
  const drafts = useDrafts()
  const outbox = useOutbound('outbox')
  const scheduled = useOutbound('scheduled', tab === 'scheduled')

  const mailboxes = me.data?.mailboxes ?? []
  const labels = useMemo(() => new Map((me.data?.labels ?? []).map((label): [string, MailLabel] => [label.id, label])), [me.data?.labels])
  const draftThreads = useMemo(() => new Set((drafts.data ?? []).flatMap((draft) => (draft.thread_id ? [draft.thread_id] : []))), [drafts.data])
  const showOutbox = (outbox.data?.length ?? 0) > 0 || tab === 'outbox'
  const mixed = mailboxes.length > 1
  const mailboxName = (id: string) => (mixed ? mailboxes.find((mailbox) => mailbox.id === id)?.name : undefined)

  const selectTab = (next: MailTab) => {
    setQuery('')
    setTyped('')
    setUnreadOnly(false)
    router.setParams({ tab: next === 'inbox' ? undefined : next })
  }
  const change = (thread: MailThreadSummary, next: ThreadChange) =>
    update.mutate({ threadId: thread.id, change: next }, { onError: (error) => Alert.alert('The change could not be saved.', errorMessage(error)) })
  const swiped = (thread: MailThreadSummary, step: SwipeStep) => {
    if ('change' in step.outcome) change(thread, step.outcome.change)
    else if (!start.isPending) start.mutate({ mode: step.outcome.reply, threadId: thread.id })
  }
  const rowMenu = (thread: MailThreadSummary) => {
    const actions = threadActions(thread.folder)
    setMenu({
      title: thread.subject || '(no subject)',
      actions: [
        ...(actions.archive ? [{ label: actions.archive === 'archive' ? 'Archive' : 'Move to inbox', icon: actions.archive === 'archive' ? ('archive' as const) : ('directInbox' as const), onPress: () => change(thread, { folder: actions.archive === 'archive' ? 'archive' : 'inbox' }) }] : []),
        { label: thread.unread ? 'Mark as read' : 'Mark as unread', icon: thread.unread ? 'mail' : 'smsTracking', onPress: () => change(thread, { read: thread.unread }) },
        { label: thread.starred ? 'Unstar' : 'Star', icon: 'star', onPress: () => change(thread, { starred: !thread.starred }) },
        ...(actions.spam ? [{ label: actions.spam === 'spam' ? 'Report spam' : 'Not spam', icon: 'shieldCross' as const, onPress: () => change(thread, { folder: actions.spam === 'spam' ? 'spam' : 'inbox' }) }] : []),
        actions.trash === 'trash'
          ? { label: 'Move to trash', icon: 'trash', destructive: true, onPress: () => change(thread, { folder: 'trash' }) }
          : { label: 'Restore', icon: 'undo', onPress: () => change(thread, { restore: true }) },
      ],
    })
  }

  // no mailbox to read: an admin sets mail up, a member asks for a mailbox
  if (me.data && mailboxes.length === 0) {
    return (
      <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
        <PaneHeader className="px-3">
          <HeaderTitle>Mail</HeaderTitle>
        </PaneHeader>
        {me.data.can_manage ? (
          <EmptyState
            icon="mail"
            title="Set up mail"
            description="Add a domain and create mailboxes, and mail for your team arrives here."
            action={<Button label="Set up mail" onPress={() => router.push('/settings/mail/domains')} />}
          />
        ) : (
          <EmptyState icon="mail" title="No mailbox yet" description="Ask an admin for a mailbox." />
        )}
      </SafeAreaView>
    )
  }

  const unreadCount = !searching && (tab === 'inbox' || tab === 'spam') ? (counts.data?.mailboxes ?? []).reduce((sum, mailbox) => sum + mailbox[tab], 0) : 0
  const threadsShown = searching ? search : list
  const rows: MailThreadSummary[] = threadsShown.data ?? []
  const entries: ListEntry[] = groupThreads(rows, { sentView: tab === 'sent', grouped: !searching })
  const empty = searching
    ? { title: 'No results', description: 'Try other words, or fewer operators.' }
    : unreadOnly
      ? { title: 'No unread conversations', description: 'You have read everything here.' }
      : TAB_INFO[tab].empty
  const showsThreads = searching || source.kind === 'threads'
  const sendQuery = source.kind === 'drafts' ? drafts : source.kind === 'outbound' ? (source.view === 'outbox' ? outbox : scheduled) : null

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="px-3">
        <HeaderTitle>Mail</HeaderTitle>
        {unreadCount > 0 ? <Text className="text-[12px] text-muted-foreground">{unreadCount > 999 ? '999+' : unreadCount} unread</Text> : null}
        <View className="flex-1" />
        {source.kind === 'threads' && !searching ? (
          <View accessibilityRole="radiogroup" accessibilityLabel="Show" className="flex-row rounded-md border border-border">
            {(['all', 'unread'] as const).map((value) => {
              const selected = (value === 'unread') === unreadOnly
              return (
                <Pressable key={value} accessibilityRole="radio" accessibilityState={{ selected }} onPress={() => setUnreadOnly(value === 'unread')} className={`h-7 justify-center px-2 ${selected ? 'bg-muted' : ''}`}>
                  <Text className={`text-[12px] ${selected ? 'text-foreground' : 'text-muted-foreground'}`}>{value === 'all' ? 'All' : 'Unread'}</Text>
                </Pressable>
              )
            })}
          </View>
        ) : null}
        <IconButton icon="edit" label="New email" disabled={start.isPending} onPress={() => start.mutate({ mode: 'new' })} />
      </PaneHeader>
      <View className="flex-row items-center gap-2 border-b border-border px-3 py-2">
        <Icon name="search" size={16} color={colors.mutedForeground} />
        <TextInput
          value={typed}
          onChangeText={setTyped}
          onSubmitEditing={() => setQuery(typed)}
          placeholder="Search mail"
          placeholderTextColor={colors.mutedForeground}
          selectionColor={colors.primary}
          keyboardAppearance={scheme}
          returnKeyType="search"
          autoCapitalize="none"
          autoCorrect={false}
          accessibilityLabel="Search mail"
          className="flex-1 min-h-8 font-noto text-[16px] text-foreground"
        />
        {typed || query ? (
          <IconButton
            icon="close"
            label="Clear the search"
            onPress={() => {
              setTyped('')
              setQuery('')
            }}
          />
        ) : null}
      </View>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} className="grow-0 border-b border-border" contentContainerClassName="min-h-10 items-center gap-1.5 px-3 py-1">
        {shownTabs(showOutbox).map((entry) => {
          const active = !searching && entry === tab
          return (
            <Pressable key={entry} accessibilityRole="tab" accessibilityState={{ selected: active }} onPress={() => selectTab(entry)} className={`h-7 justify-center rounded-md px-2.5 ${active ? 'bg-primary/10' : 'active:bg-muted'}`}>
              <Text className={`text-[13px] ${active ? 'text-primary' : 'text-muted-foreground'}`}>{TAB_INFO[entry].label}</Text>
            </Pressable>
          )
        })}
      </ScrollView>
      <Notices mailboxes={mailboxes} canManage={me.data?.can_manage ?? false} />
      {showsThreads ? (
        !threadsShown.data ? (
          threadsShown.isError ? (
            <ErrorView error={threadsShown.error} onRetry={() => void threadsShown.refetch()} />
          ) : (
            <LoadingView />
          )
        ) : (
          <FlatList
            data={entries}
            keyExtractor={(entry) => entry.key}
            contentContainerClassName={entries.length === 0 ? 'grow' : undefined}
            refreshControl={<PullRefresh onRefresh={() => Promise.all([threadsShown.refetch(), counts.refetch(), drafts.refetch()])} />}
            onEndReachedThreshold={0.5}
            onEndReached={() => {
              if (threadsShown.hasNextPage && !threadsShown.isFetchingNextPage) void threadsShown.fetchNextPage()
            }}
            ListFooterComponent={threadsShown.isFetchingNextPage ? <View className="py-3"><Spinner /></View> : undefined}
            ListEmptyComponent={<EmptyState icon={searching ? 'search' : TAB_INFO[tab].icon} {...empty} />}
            renderItem={({ item }) =>
              item.kind === 'group' ? (
                <Text className="h-[30px] px-4 pb-1 pt-3 font-noto-medium text-[11px] uppercase tracking-wide text-muted-foreground">{item.label}</Text>
              ) : (
                <ThreadRow
                  thread={item.thread}
                  labels={labels}
                  sentView={tab === 'sent' && !searching}
                  draft={draftThreads.has(item.thread.id)}
                  mailboxName={mailboxName(item.thread.mailbox_id)}
                  onOpen={(thread) => router.push(`/mail/${thread.id}`)}
                  onStar={(thread) => change(thread, { starred: !thread.starred })}
                  onLongPress={rowMenu}
                  swipes={swipes}
                  onSwipe={swiped}
                />
              )
            }
          />
        )
      ) : sendQuery && !sendQuery.data ? (
        sendQuery.isError ? <ErrorView error={sendQuery.error} onRetry={() => void sendQuery.refetch()} /> : <LoadingView />
      ) : source.kind === 'drafts' ? (
        <SendList items={[...(drafts.data ?? [])].sort((a, b) => b.updated_at.localeCompare(a.updated_at))} empty={TAB_INFO.drafts.empty} icon="edit" onRefresh={() => drafts.refetch()} render={(draft: MailDraft) => <DraftRow draft={draft} />} />
      ) : source.kind === 'outbound' ? (
        <SendList
          items={(source.view === 'outbox' ? outbox.data : scheduled.data) ?? []}
          empty={TAB_INFO[tab].empty}
          icon={TAB_INFO[tab].icon}
          onRefresh={() => (source.view === 'outbox' ? outbox.refetch() : scheduled.refetch())}
          render={(item: MailOutboundItem) => <OutboundRow item={item} scheduled={source.view === 'scheduled'} />}
        />
      ) : null}
      <UndoSendBar />
      <ActionSheet visible={menu !== null} title={menu?.title} actions={menu?.actions ?? []} onClose={() => setMenu(null)} />
    </SafeAreaView>
  )
}

/** Over the list: sending is paused (owners and admins), or a mailbox is suspended. */
function Notices({ mailboxes, canManage }: { mailboxes: { status: string; name: string; primary_address?: string | null }[]; canManage: boolean }) {
  const { colors } = useTheme()
  const policy = useSendPolicy()
  const suspended = mailboxes.filter((mailbox) => mailbox.status === 'suspended')
  const notices: { key: string; title: string; text: string; resume?: boolean }[] = []
  if (policy.data?.paused_at) {
    notices.push({ key: 'paused', title: 'Sending is paused for this workspace', text: `${policy.data.pause_reason ? `${policy.data.pause_reason} ` : ''}New messages wait in the Outbox. Ask the instance admin to resume sending.` })
  }
  if (suspended.length > 0) {
    const [first] = suspended
    const one = suspended.length === 1
    notices.push({
      key: 'suspended',
      title: one ? `${first!.name} is suspended` : `${suspended.length} mailboxes are suspended`,
      text: `${one && first!.primary_address ? `New mail to ${first!.primary_address} is refused.` : 'New mail to them is refused.'} The mail already here stays readable.${canManage ? '' : ' Ask an admin to resume it.'}`,
      resume: canManage,
    })
  }
  if (notices.length === 0) return null
  return (
    <View className="gap-2 p-3">
      {notices.map((notice) => (
        <View key={notice.key} accessibilityRole="alert" className="flex-row gap-2.5 rounded-lg border border-border bg-card px-3 py-2.5">
          <Icon name="pauseCircle" size={16} color={colors.foreground} />
          <View className="flex-1 gap-0.5">
            <Text className="font-noto-medium text-[13px] leading-[18px]">{notice.title}</Text>
            <Text className="text-[13px] leading-[18px] text-muted-foreground">{notice.text}</Text>
          </View>
          {notice.resume ? <Button label="Resume" variant="outline" size="sm" onPress={() => router.push('/settings/mail/mailboxes')} /> : null}
        </View>
      ))}
    </View>
  )
}

function SendList<T extends { id: string }>({ items, empty, icon, onRefresh, render }: { items: T[]; empty: { title: string; description: string }; icon: (typeof TAB_INFO)[MailTab]['icon']; onRefresh: () => Promise<unknown>; render: (item: T) => ReactElement }) {
  return (
    <FlatList
      data={items}
      keyExtractor={(item) => item.id}
      contentContainerClassName={items.length === 0 ? 'grow' : undefined}
      refreshControl={<PullRefresh onRefresh={onRefresh} />}
      ListEmptyComponent={<EmptyState icon={icon} {...empty} />}
      renderItem={({ item }) => render(item)}
    />
  )
}

/** A draft: a tap continues it, the bin deletes it. */
function DraftRow({ draft }: { draft: MailDraft }) {
  const { client, workspaceId } = useOrbit()
  const { colors } = useTheme()
  const queryClient = useQueryClient()
  const remove = useMutation({ mutationFn: () => mail.removeDraft(client, workspaceId, draft.id), onSettled: () => refreshSendViews(queryClient, workspaceId) })
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`Draft: ${draft.subject || '(no subject)'}`}
      onPress={() => {
        // the list follows the live topic: its copy is the newest one
        queryClient.setQueryData(mailKeys.draft(workspaceId, draft.id), draft)
        router.push({ pathname: '/mail/compose', params: { draft: draft.id } })
      }}
      className="flex-row items-start gap-2 border-b border-border px-3 py-2.5 active:bg-muted"
    >
      <View className="flex-1 min-w-0 gap-0.5">
        <View className="flex-row items-baseline gap-2">
          <Text className="font-noto-semibold text-[13px] text-destructive">Draft</Text>
          <Text numberOfLines={1} className="shrink text-[13px]">
            {recipientSummary(draft.to, draft.cc, draft.bcc)}
          </Text>
          <View className="flex-1" />
          <Text className="text-[12px] text-muted-foreground/70">{relativeTime(draft.updated_at)}</Text>
        </View>
        <Text numberOfLines={1} className="text-[13px]">
          {draft.subject || '(no subject)'}
        </Text>
        <Text numberOfLines={1} className="text-[12px] text-muted-foreground/70">
          {htmlToText(draft.body_html).replace(/\s+/g, ' ') || ' '}
        </Text>
      </View>
      <IconButton
        icon="trash"
        label="Delete draft"
        color={alpha(colors.mutedForeground, 0.7)}
        disabled={remove.isPending}
        onPress={async () => {
          if (!(await confirmAction('Delete this draft?', 'Its text and files are deleted.', 'Delete'))) return
          remove.mutate(undefined, { onError: (error) => Alert.alert('The draft could not be deleted.', errorMessage(error)) })
        }}
      />
    </Pressable>
  )
}

/** A message that waits to go: when it goes or why it waits; Cancel send brings it back as a draft. */
function OutboundRow({ item, scheduled }: { item: MailOutboundItem; scheduled: boolean }) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const status = outboxStatus(item, new Date())
  const text = scheduled && !item.hold_reason && new Date(item.release_at) > new Date() ? `Sends ${fullDate(item.release_at)}, ${timeOfDay(item.release_at)}` : status.text
  const cancel = useMutation({
    mutationFn: () => mail.cancelOutbound(client, workspaceId, item.id),
    onSuccess: (draft) => {
      queryClient.setQueryData(mailKeys.draft(workspaceId, draft.id), draft)
      router.push({ pathname: '/mail/compose', params: { draft: draft.id } })
    },
    onError: (error) => Alert.alert(error instanceof ApiProblem && error.code === 'already_sent' ? 'Too late: the message was already sent.' : 'The message could not be cancelled.', error instanceof ApiProblem && error.code === 'already_sent' ? undefined : errorMessage(error)),
    onSettled: () => refreshSendViews(queryClient, workspaceId),
  })
  return (
    <View className="flex-row items-start gap-2 border-b border-border px-3 py-2.5">
      <View className="flex-1 min-w-0 gap-0.5">
        <Text numberOfLines={1} className="text-[13px]">
          {recipientSummary(item.to, [], [])}
        </Text>
        <Text numberOfLines={1} className="text-[13px]">
          {item.subject || '(no subject)'}
        </Text>
        <Text className={`text-[12px] ${status.problem ? 'text-destructive' : 'text-muted-foreground/70'}`}>{text}</Text>
      </View>
      {status.canCancel ? <Button label="Cancel send" variant="outline" size="xs" loading={cancel.isPending} onPress={() => cancel.mutate()} /> : null}
    </View>
  )
}
