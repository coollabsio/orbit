import { useQueryClient } from '@tanstack/react-query'
import { applyChatActions, applyChatEvents, chatQueryKeys } from '../../chat-cache'
import { router } from 'expo-router'
import { type ReactElement, useMemo, useRef, useState } from 'react'
import { FlatList, Platform, Pressable, Share, View } from 'react-native'

import { decodeMentions, hasReaction, type MessageAction, messageActions, type MessageRow, type Person, quickReactions, type ThreadPages, toggleReaction, upsertThreadMessage } from '../../data/chat'
import * as chat from '../../data/chat-api'
import { dayLabel } from '../../data/format'
import { markdownToText } from '../../data/markdown'
import type { MessageRecord } from '../../data/orbit'
import { confirmAction, showError, useChatWrite, useIsChatAdmin } from '../../hooks/chat'

import { useOrbit } from '../../orbit-context'
import { useConversations, usePeople } from '../../queries'
import { type IconName, Spinner, Text } from '../../ui'
import type { ComposerState, PendingMessage } from './Composer'
import { Emoji, EmojiPicker } from './EmojiPicker'
import { type MessageHandlers, MessageItem } from './Message'
import { ActionSheet, type SheetItem } from './Sheet'

export type ListRow = MessageRow | { type: 'pending'; key: string; pending: PendingMessage }

/** Names and shown text for message rows: members by id, mentions decoded, markdown flattened. */
export function useMessageText() {
  const people = usePeople()
  const conversations = useConversations()
  const channels: Person[] = useMemo(() => (conversations.data ?? []).map((item) => ({ id: item.id, name: item.name })), [conversations.data])
  return {
    nameOf: (userId: string) => people.find((person) => person.id === userId)?.name ?? 'Unknown',
    text: (body: string) => markdownToText(decodeMentions(body, people, channels)),
  }
}

const ACTION_LOOK: Record<MessageAction, { label: string; icon: IconName; destructive?: boolean }> = {
  reply: { label: 'Reply', icon: 'reply' },
  thread: { label: 'Reply in thread', icon: 'messages2' },
  copy: { label: Platform.OS === 'web' ? 'Copy text' : 'Copy or share text', icon: 'copy' },
  pin: { label: 'Pin', icon: 'pin' },
  unpin: { label: 'Unpin', icon: 'pinOff' },
  unread: { label: 'Mark as unread', icon: 'unread' },
  edit: { label: 'Edit', icon: 'edit' },
  delete: { label: 'Delete', icon: 'trash', destructive: true },
}

/**
 * The long-press sheet of a message and the reaction picker: quick reactions first, then the web's actions. Reactions
 * show at once and roll back when the server refuses them.
 */
export function useMessageSheet({ inThread, composer, loaded, onMarkedUnread }: { inThread: boolean; composer: ComposerState | null; loaded: readonly MessageRecord[]; onMarkedUnread?: () => void }) {
  const { user, workspaceId, client } = useOrbit()
  const queryClient = useQueryClient()
  const moderator = useIsChatAdmin()
  const run = useChatWrite()
  const words = useMessageText()
  const [selected, setSelected] = useState<MessageRecord | null>(null)
  const [picking, setPicking] = useState<MessageRecord | null>(null)

  const put = (message: MessageRecord) => {
    applyChatActions(queryClient, workspaceId, [{ kind: 'upsert-message', conversationId: message.conversation_id, message }])
    const rootId = message.thread_root_id ?? message.id
    queryClient.setQueryData<ThreadPages>(chatQueryKeys.thread(workspaceId, rootId), (data) => data && upsertThreadMessage(data, message))
  }

  const react = (message: MessageRecord, emoji: string) => {
    const on = !hasReaction(message, emoji, user.id)
    put(toggleReaction(message, emoji, user.id))
    chat.react(client, workspaceId, message.id, emoji, on).then(
      ({ result, events }) => {
        applyChatEvents(queryClient, workspaceId, events)
        put(result)
      },
      (error) => {
        put(message)
        showError('Could not update the reaction', error)
      },
    )
  }

  const remove = async (message: MessageRecord) => {
    const confirmed = await confirmAction('Delete message?', message.reply_count > 0 ? 'The replies stay in the thread. This cannot be undone.' : 'This cannot be undone.', 'Delete')
    if (confirmed) await run('Could not delete the message', (client, workspaceId) => chat.deleteMessage(client, workspaceId, message.id))
  }

  const copy = (message: MessageRecord) => {
    const text = words.text(message.body)
    if (Platform.OS === 'web') void navigator.clipboard.writeText(text)
    else void Share.share({ message: text })
  }

  const runAction = (action: MessageAction, message: MessageRecord) => {
    switch (action) {
      case 'reply':
        return composer?.reply(message)
      case 'thread':
        return router.push(`/chat/thread/${message.thread_root_id ?? message.id}`)
      case 'copy':
        return copy(message)
      case 'pin':
      case 'unpin':
        return void run(action === 'pin' ? 'Could not pin the message' : 'Could not unpin the message', (client, workspaceId) => chat.setPinned(client, workspaceId, message.id, action === 'pin'))
      case 'unread':
        onMarkedUnread?.()
        return void run('Could not mark as unread', (client, workspaceId) => chat.markUnread(client, workspaceId, message.id))
      case 'edit':
        return composer?.edit(message)
      case 'delete':
        return void remove(message)
    }
  }

  const items: SheetItem[] = selected
    ? messageActions(selected, user.id, moderator, { inThread, canWrite: composer !== null }).map((action) => ({ key: action, ...ACTION_LOOK[action], onPress: () => runAction(action, selected) }))
    : []
  const quick = quickReactions(loaded, user.id, 5)

  const sheet = (
    <>
      <ActionSheet
        visible={selected !== null}
        items={items}
        onClose={() => setSelected(null)}
        header={
          selected && !selected.deleted && selected.kind === 'message' ? (
            <QuickReactions
              emojis={quick}
              onReact={(emoji) => {
                react(selected, emoji)
                setSelected(null)
              }}
              onMore={() => {
                setPicking(selected)
                setSelected(null)
              }}
            />
          ) : undefined
        }
      />
      <EmojiPicker visible={picking !== null} onPick={(emoji) => picking && react(picking, emoji)} onClose={() => setPicking(null)} />
    </>
  )

  const handlers: MessageHandlers = {
    ...words,
    onLongPress: (message) => {
      if (message.kind === 'message') setSelected(message)
    },
    onReact: react,
    onAddReaction: setPicking,
    onOpenThread: inThread ? undefined : (message) => router.push(`/chat/thread/${message.id}`),
  }

  return { sheet, handlers }
}

function QuickReactions({ emojis, onReact, onMore }: { emojis: string[]; onReact: (emoji: string) => void; onMore: () => void }) {
  return (
    <View className="flex-row justify-around gap-2 px-2 pt-1 pb-3 border-b border-border">
      {emojis.map((emoji) => (
        <Pressable key={emoji} accessibilityRole="button" accessibilityLabel={`React with ${emoji}`} onPress={() => onReact(emoji)} className="size-11 items-center justify-center rounded-full bg-secondary active:bg-muted">
          <Emoji value={emoji} size={24} />
        </Pressable>
      ))}
      <Pressable accessibilityRole="button" accessibilityLabel="More reactions" onPress={onMore} className="size-11 items-center justify-center rounded-full bg-secondary active:bg-muted">
        <Text className="text-[20px] text-muted-foreground">+</Text>
      </Pressable>
    </View>
  )
}

function DayChip({ label }: { label: string }) {
  return (
    <View accessibilityRole="header" accessibilityLabel={label} className="items-center py-2">
      <View className="px-3 py-0.5 rounded-full border border-border bg-card">
        <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground">{label}</Text>
      </View>
    </View>
  )
}

/**
 * The messages, newest at the bottom (an inverted list: `rows` come newest first). Reaching the top loads older pages;
 * reaching the bottom loads newer ones when the list does not start at the present (a search result).
 */
export function MessageList({
  rows,
  handlers,
  highlightedId,
  loadingOlder,
  loadingNewer,
  onOlder,
  onNewer,
  start,
  end,
  fromTop = false,
}: {
  rows: readonly ListRow[]
  handlers: MessageHandlers
  highlightedId?: string
  loadingOlder: boolean
  loadingNewer?: boolean
  onOlder: () => void
  onNewer?: () => void
  /** Above the oldest message: the conversation start or the thread root. */
  start?: ReactElement
  /** Under the newest message. */
  end?: ReactElement
  /** A short list sits at the top (a thread), not at the composer. */
  fromTop?: boolean
}) {
  const list = useRef<FlatList<ListRow>>(null)
  const highlightIndex = highlightedId ? rows.findIndex((row) => row.type === 'message' && row.message.id === highlightedId) : -1
  return (
    <FlatList
      ref={list}
      inverted
      data={rows}
      keyExtractor={(row) => row.key}
      keyboardShouldPersistTaps="handled"
      keyboardDismissMode="interactive"
      // the list is flipped: `flex-end` is the top of the screen
      contentContainerClassName={`pt-3 ${fromTop ? 'grow justify-end' : ''}`}
      initialScrollIndex={highlightIndex > 0 ? highlightIndex : undefined}
      onScrollToIndexFailed={({ index, averageItemLength }) => {
        list.current?.scrollToOffset({ offset: index * averageItemLength, animated: false })
        setTimeout(() => list.current?.scrollToIndex({ index, viewPosition: 0.5, animated: false }), 100)
      }}
      onEndReached={onOlder}
      onEndReachedThreshold={0.5}
      onStartReached={onNewer}
      onStartReachedThreshold={0.5}
      ListHeaderComponent={loadingNewer ? <View className="p-4"><Spinner /></View> : end}
      ListFooterComponent={loadingOlder ? <View className="p-4"><Spinner /></View> : start}
      renderItem={({ item }) => {
        if (item.type === 'day') return <DayChip label={dayLabel(item.at)} />
        if (item.type === 'pending') return <PendingRow pending={item.pending} text={handlers.text} />
        return <MessageItem message={item.message} groupStart={item.groupStart} highlighted={item.message.id === highlightedId} handlers={handlers} />
      }}
    />
  )
}

function PendingRow({ pending, text }: { pending: PendingMessage; text: (body: string) => string }) {
  return (
    <View className="pr-3 pl-12.5 py-0.5 opacity-50">
      {pending.body ? <Text className="text-[14px] leading-[21px]">{text(pending.body)}</Text> : null}
      {pending.files > 0 ? <Text className="text-[12px] text-muted-foreground">Sending {pending.files === 1 ? '1 file' : `${pending.files} files`}…</Text> : null}
    </View>
  )
}
