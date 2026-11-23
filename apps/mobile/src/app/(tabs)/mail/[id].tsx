import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useRef, useState } from 'react'
import { Alert, Pressable, ScrollView, View } from 'react-native'

import { MessageItem } from '../../../components/mail/MessageItem'
import { UndoSendBar } from '../../../components/mail/UndoSendBar'
import { ActionSheet, type SheetAction } from '../../../components/settings/Dialog'
import { folderLabel, foldedRange, threadActions, type ThreadChange } from '../../../data/mail'
import { useMailMe, useStartDraft, useThread, useUpdateThread } from '../../../hooks/mail'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Button, EmptyState, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../../ui'

/** One conversation (the web's reading pane at phone size): its actions, its messages oldest first, and Reply. */
export default function MailThreadScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { colors } = useTheme()
  const detail = useThread(id)
  const me = useMailMe()
  const update = useUpdateThread()
  const start = useStartDraft()
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  const [showAll, setShowAll] = useState(false)
  // one sheet: the "…" actions, or the labels it opens (a second Modal would not open while the first closes)
  const [sheet, setSheet] = useState<'actions' | 'labels' | null>(null)
  const muted = alpha(colors.mutedForeground, 0.7)
  const back = () => (router.canGoBack() ? router.back() : router.replace('/mail'))

  // Opening a thread reads it, and a newer message is read again; "Mark as unread" leaves for the list.
  const readKey = useRef<string | null>(null)
  const shown = detail.data?.thread
  useEffect(() => {
    if (!shown) return
    const key = `${shown.id}:${shown.last_message_at}`
    if (readKey.current === key) return
    readKey.current = key
    if (shown.unread) update.mutate({ threadId: shown.id, change: { read: true } })
  }, [shown, update])

  if (!detail.data) {
    return (
      <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
        <PaneHeader className="px-2">
          <IconButton icon="back" label="Back" onPress={back} />
        </PaneHeader>
        {detail.isError ? (
          <EmptyState icon="mail" title="This conversation could not be loaded" description="It may have been deleted, or the connection failed." action={<Button label="Try again" variant="outline" size="sm" onPress={() => void detail.refetch()} />} />
        ) : (
          <LoadingView />
        )}
      </SafeAreaView>
    )
  }

  const { thread, messages } = detail.data
  const subject = thread.subject || '(no subject)'
  const actions = threadActions(thread.folder)
  const labels = me.data?.labels ?? []
  const chips = labels.filter((label) => thread.labels.includes(label.id))
  const change = (next: ThreadChange, leaves: boolean) => {
    update.mutate({ threadId: thread.id, change: next }, { onError: (error) => Alert.alert('The change could not be saved.', errorMessage(error)) })
    if (leaves) back()
  }
  const lastIndex = messages.length - 1
  const range = showAll ? null : foldedRange(messages.length)
  const fold = range && !messages.slice(range[0], range[1]).some((message) => expanded[message.id]) ? range : null
  const replyAll = (messages.at(-1)?.to.length ?? 0) + (messages.at(-1)?.cc.length ?? 0) > 1
  const labelActions: SheetAction[] = labels.length
    ? labels.map((label) => {
        const on = thread.labels.includes(label.id)
        return { label: label.name, icon: on ? 'checkSquare' : 'square', onPress: () => change(on ? { remove_labels: [label.id] } : { add_labels: [label.id] }, false) }
      })
    : [me.data?.can_manage
        ? { label: 'Create labels in Settings', icon: 'tag', onPress: () => router.push('/settings/mail/labels') }
        : { label: 'No labels yet', disabled: true, onPress: () => {} }]

  const menuActions: SheetAction[] = [
    { label: thread.starred ? 'Unstar' : 'Star', icon: thread.starred ? 'starFilled' : 'star', onPress: () => change({ starred: !thread.starred }, false) },
    { label: 'Mark as unread', icon: 'mail', onPress: () => change({ read: false }, true) },
    { label: 'Labels', icon: 'tag', onPress: () => setSheet('labels') },
  ]
  if (actions.archive) {
    const archive = actions.archive === 'archive'
    menuActions.push({ label: archive ? 'Archive' : 'Move to inbox', icon: archive ? 'archive' : 'directInbox', onPress: () => change({ folder: archive ? 'archive' : 'inbox' }, true) })
  }
  if (actions.spam) {
    const spam = actions.spam === 'spam'
    menuActions.push({ label: spam ? 'Report spam' : 'Not spam', icon: spam ? 'shieldCross' : 'directInbox', onPress: () => change({ folder: spam ? 'spam' : 'inbox' }, true) })
  }
  menuActions.push(
    actions.trash === 'trash'
      ? { label: 'Move to trash', icon: 'trash', destructive: true, onPress: () => change({ folder: 'trash' }, true) }
      : { label: 'Restore', icon: 'undo', onPress: () => change({ restore: true }, true) },
  )

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="px-2 gap-0.5">
        <IconButton icon="back" label="Back" onPress={back} />
        {/* the subject and folder sit in the header row, so the messages start one row higher */}
        <View className="min-w-0 flex-1 flex-row items-center gap-2">
          <Text accessibilityRole="header" numberOfLines={1} className="shrink font-noto-semibold text-[15px]">
            {subject}
          </Text>
          <View className="shrink-0 rounded-md bg-secondary px-2 py-0.5">
            <Text className="font-noto-medium text-[12px] text-secondary-foreground">{folderLabel(thread.folder)}</Text>
          </View>
        </View>
        <IconButton icon="more" label="Thread actions" color={muted} onPress={() => setSheet('actions')} />
      </PaneHeader>
      <ScrollView contentContainerClassName="px-4 pt-3 pb-8">
        {chips.length ? (
        <View className="flex-row flex-wrap items-center gap-2.5 pb-2">
          {chips.map((label) => (
            <View key={label.id} className="flex-row items-center gap-1 rounded-md border border-border px-2 py-0.5">
              <View className="size-1.5 rounded-full" style={{ backgroundColor: label.color }} />
              <Text className="text-[12px]">{label.name}</Text>
            </View>
          ))}
        </View>
        ) : null}
        {thread.folder === 'spam' ? (
          <FolderNotice title="In Spam" text="Links and images in spam can be harmful. Move it to the inbox only if you trust the sender." action="Not spam" onPress={() => change({ folder: 'inbox' }, true)} />
        ) : null}
        {thread.folder === 'trash' ? <FolderNotice title="In Trash" text="Conversations in the trash are deleted for good after 30 days." action="Restore" onPress={() => change({ restore: true }, true)} /> : null}
        {messages.map((message, index) => {
          if (fold && index >= fold[0] && index < fold[1]) {
            return index === fold[0] ? (
              <Pressable key="fold" accessibilityRole="button" onPress={() => setShowAll(true)} className="my-1 h-8 items-center justify-center">
                <View className="absolute inset-x-0 top-[50%] border-t border-dashed border-border" />
                <View className="rounded-full border border-border bg-background px-3 py-1">
                  <Text className="text-[12px] text-muted-foreground">{fold[1] - fold[0]} more messages</Text>
                </View>
              </Pressable>
            ) : null
          }
          const open = expanded[message.id] ?? index === lastIndex
          return <MessageItem key={message.id} message={message} expanded={open} onToggle={() => setExpanded((previous) => ({ ...previous, [message.id]: !open }))} />
        })}
        <View className="mt-4 flex-row flex-wrap gap-2">
          <Button label="Reply" icon="reply" variant="outline" disabled={start.isPending} onPress={() => start.mutate({ mode: 'reply', threadId: thread.id })} />
          {replyAll ? <Button label="Reply all" icon="reply" variant="outline" disabled={start.isPending} onPress={() => start.mutate({ mode: 'reply_all', threadId: thread.id })} /> : null}
          <Button label="Forward" icon="forward" variant="outline" disabled={start.isPending} onPress={() => start.mutate({ mode: 'forward', threadId: thread.id })} />
        </View>
      </ScrollView>
      <UndoSendBar />
      <ActionSheet
        visible={sheet !== null}
        title={sheet === 'labels' ? 'Labels' : undefined}
        actions={sheet === 'labels' ? labelActions : menuActions}
        onClose={() => setSheet(null)}
      />
    </SafeAreaView>
  )
}

function FolderNotice({ title, text, action, onPress }: { title: string; text: string; action: string; onPress: () => void }) {
  return (
    <View accessibilityRole="alert" className="mt-2 mb-2 gap-2 rounded-lg border border-border bg-card px-3 py-2.5">
      <Text className="font-noto-medium text-[13px]">{title}</Text>
      <Text className="text-[13px] leading-[18px] text-muted-foreground">{text}</Text>
      <View className="flex-row">
        <Button label={action} variant="outline" size="sm" onPress={onPress} />
      </View>
    </View>
  )
}
