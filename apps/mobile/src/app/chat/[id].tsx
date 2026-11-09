import { useQueryClient } from '@tanstack/react-query'
import { applyChatEvents } from '../../chat-cache'
import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useMemo, useRef, useState } from 'react'
import { View } from 'react-native'

import { Composer, useComposer } from '../../components/chat/Composer'
import { ConversationOptions } from '../../components/chat/ConversationOptions'
import { type ListRow, MessageList, useMessageSheet } from '../../components/chat/MessageList'
import { buildMessageRows, conversationTitle, hasGuests } from '../../data/chat'
import * as chat from '../../data/chat-api'
import { fullDate, timeOfDay } from '../../data/format'
import type { ConversationRecord } from '../../data/orbit'
import { useChatWrite, useMessages } from '../../hooks/chat'
import { useIsGuest, useOrbit } from '../../orbit-context'
import { useChatStates, useConversations, useMembers, usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { Avatar, Button, ErrorView, Icon, IconButton, LoadingView, KeyboardSpace, SafeAreaView, Text } from '../../ui'
import { useShownPresence } from '../../components/chat/Pane'

/**
 * A conversation on a phone, painted on the card surface like the web. `?message=` opens it around that message (a
 * search result) with the message marked; "Jump to present" goes back to the newest.
 */
export default function ConversationScreen() {
  const { id, message: around } = useLocalSearchParams<{ id: string; message?: string }>()
  const { client, workspaceId, user } = useOrbit()
  const queryClient = useQueryClient()
  const { colors } = useTheme()
  const shown = useShownPresence()
  const guest = useIsGuest()
  const people = usePeople()
  const members = useMembers().data
  const conversations = useConversations()
  const states = useChatStates()
  const run = useChatWrite()
  const [options, setOptions] = useState(false)
  /** After "Mark as unread" the open conversation stops reading itself until a new message arrives. */
  const [readPausedAt, setReadPausedAt] = useState<string | null>(null)

  const messages = useMessages(id, around)
  const composer = useComposer(id, null)
  const conversation = conversations.data?.find((item) => item.id === id)
  const loaded = useMemo(() => messages.data?.pages.flatMap((page) => page.items) ?? [], [messages.data])
  const lastReadId = states.data?.find((state) => state.conversation_id === id)?.last_read_message_id
  // The web's "New" line: the read cursor when the conversation opened stays while it is open; after "Mark as unread", the live one.
  const opened = useRef<{ cursor: string | null | undefined } | null>(null)
  if (!opened.current && states.data) {
    const state = states.data.find((item) => item.conversation_id === id)
    opened.current = { cursor: state && state.unread_count > 0 ? state.last_read_message_id : undefined }
  }
  const cursor = readPausedAt !== null ? lastReadId : opened.current?.cursor
  // the first unread message is older than the loaded window: a "New" line at its top would be wrong
  const unreadBeforeWindow = typeof cursor === 'string' && messages.hasPreviousPage && loaded.length > 0 && loaded[0]!.id > cursor
  const { sheet, handlers } = useMessageSheet({ inThread: false, composer, loaded, onMarkedUnread: () => setReadPausedAt(loaded.at(-1)?.id ?? '') })
  const rows: ListRow[] = useMemo(
    () => [
      ...composer.pending
        // the live echo can arrive before the response: then the message is already in the list
        .filter((item) => !loaded.some((message) => message.nonce === item.nonce))
        .map((item): ListRow => ({ type: 'pending', key: item.nonce, pending: item }))
        .reverse(),
      ...buildMessageRows(loaded, { lastReadMessageId: unreadBeforeWindow ? undefined : cursor, userId: user.id }).reverse(),
    ],
    [loaded, composer.pending, unreadBeforeWindow, cursor, user.id],
  )

  // Read up to the newest message when the conversation opens and whenever a new one arrives while it is open.
  const atPresent = messages.data !== undefined && !messages.hasNextPage
  const newestId = atPresent ? loaded.at(-1)?.id : undefined
  useEffect(() => {
    if (!newestId || newestId === lastReadId || newestId === readPausedAt) return
    chat.markRead(client, workspaceId, id).then(
      ({ events }) => applyChatEvents(queryClient, workspaceId, events),
      () => undefined,
    )
  }, [client, id, lastReadId, newestId, queryClient, readPausedAt, workspaceId])

  const name = conversation ? conversationTitle(conversation, people, user.id) : ''
  const dmPeer = conversation?.kind === 'dm' ? (conversation.member_ids.find((member) => member !== user.id) ?? user.id) : null
  const placeholder = conversation ? `Message ${conversation.kind === 'dm' ? name : `#${name}`}` : 'Message'
  const member = conversation?.member_ids.includes(user.id) ?? false
  const back = () => (router.canGoBack() ? router.back() : router.navigate('/chat'))

  return (
    <SafeAreaView edges={['top', 'left', 'right', 'bottom']} className="flex-1 bg-card">
      <View className="h-12 flex-row items-center gap-1 px-2">
        <IconButton icon="back" label="Back to chat" size={32} iconSize={20} color={colors.mutedForeground} onPress={back} />
        <View className="flex-1 flex-row items-center gap-2 min-w-0">
          {dmPeer ? <Avatar id={dmPeer} name={name} size={20} presence={dmPeer !== user.id && conversation?.member_ids.length === 2 ? shown(dmPeer) : undefined} surface={colors.card} /> : <Icon name={conversation?.kind === 'private' ? 'lockFilled' : 'hashtagFilled'} size={18} color={colors.mutedForeground} />}
          <Text numberOfLines={1} className="font-noto-semibold text-[14px] shrink">
            {name}
          </Text>
        </View>
        <View className="flex-row gap-0.5">
          {/* the web's `size={20}` loses to the button's svg `size-4`: 16px */}
          <IconButton icon="search" label="Search" size={32} iconSize={16} color={colors.mutedForeground} onPress={() => router.push(`/chat/${id}/search`)} />
          <IconButton icon="people" label="Members" size={32} iconSize={16} color={colors.mutedForeground} onPress={() => router.push(`/chat/${id}/members`)} />
          <IconButton icon="more" label={`Options for ${name}`} size={32} iconSize={16} color={colors.mutedForeground} disabled={!conversation} onPress={() => setOptions(true)} />
        </View>
      </View>
      <KeyboardSpace className="flex-1">
        {!messages.data ? (
          messages.isError ? (
            <ErrorView error={messages.error} onRetry={() => void messages.refetch()} />
          ) : (
            <LoadingView />
          )
        ) : (
          <MessageList
            rows={rows}
            handlers={handlers}
            highlightedId={around}
            loadingOlder={messages.isFetchingPreviousPage}
            loadingNewer={messages.isFetchingNextPage}
            onOlder={() => {
              if (messages.hasPreviousPage && !messages.isFetchingPreviousPage) void messages.fetchPreviousPage()
            }}
            onNewer={() => {
              if (messages.hasNextPage && !messages.isFetchingNextPage) void messages.fetchNextPage()
            }}
            start={
              !messages.hasPreviousPage && conversation ? (
                <ConversationStart conversation={conversation} title={name} creator={handlers.nameOf(conversation.created_by)} hasGuests={hasGuests(conversation, members ?? [])} onAddPeople={member && !conversation.archived && !guest ? () => router.push(`/chat/${id}/members`) : undefined} />
              ) : undefined
            }
          />
        )}
        {around ? (
          <View className="items-center pt-1.5">
            <Button label="Jump to present" size="sm" variant="secondary" icon="arrowDown" onPress={() => router.setParams({ message: undefined })} />
          </View>
        ) : null}
        {!conversation ? null : conversation.archived ? (
          <Notice text="This channel is archived. Its messages stay searchable." />
        ) : member ? (
          <Composer state={composer} placeholder={placeholder} nameOf={handlers.nameOf} />
        ) : (
          <View className="m-2 p-3 gap-2 items-center rounded-lg border border-border">
            <Text className="text-[14px] text-muted-foreground">You are viewing #{conversation.name}.</Text>
            <Button label="Join channel" onPress={() => void run('Could not join the channel', (client, workspaceId) => chat.joinChannel(client, workspaceId, conversation.id))} />
          </View>
        )}
      </KeyboardSpace>
      {sheet}
      {conversation ? <ConversationOptions conversation={conversation} title={name} placement="header" visible={options} onClose={() => setOptions(false)} /> : null}
    </SafeAreaView>
  )
}

function Notice({ text }: { text: string }) {
  return (
    <View className="m-2 p-3 rounded-lg bg-muted">
      <Text className="text-[13px] text-center text-muted-foreground">{text}</Text>
    </View>
  )
}

/** The block above the first message: who made the channel and when, and "Add people" (the members screen) for its members. */
function ConversationStart({ conversation, title, creator, hasGuests, onAddPeople }: { conversation: ConversationRecord; title: string; creator: string; hasGuests: boolean; onAddPeople?: () => void }) {
  if (conversation.self_dm) {
    return (
      <View className="px-3 pt-8 pb-4 gap-1 items-start">
        <Text className="font-noto-semibold text-[16px] leading-[24px]">This is your space</Text>
        <Text className="text-[14px] leading-[20px] text-muted-foreground">Draft messages, keep links and files handy, or jot down notes. Only you can see it.</Text>
      </View>
    )
  }
  if (conversation.kind === 'dm') {
    return (
      <View className="px-3 pt-8 pb-4">
        <Text className="font-noto-semibold text-[16px] leading-[24px]">This is the start of your conversation with {title}</Text>
      </View>
    )
  }
  return (
    <View className="px-3 pt-8 pb-4 gap-1 items-start">
      <Text className="font-noto-semibold text-[16px] leading-[24px]">This is the start of #{title}</Text>
      {conversation.topic ? <Text className="text-[14px] text-muted-foreground">{conversation.topic}</Text> : null}
      <Text className="text-[12px] leading-[16px] text-muted-foreground">
        Created by <Text className="font-noto-medium text-[12px] leading-[16px] text-foreground/85">{creator}</Text> on {fullDate(conversation.created_at)}, {timeOfDay(conversation.created_at)}
      </Text>
      {/* the web shows "Has guests" in the header; the phone's header has no room, so the start says it */}
      {hasGuests ? <Text className="text-[12px] leading-[16px] text-muted-foreground">This channel has guests: people from outside the workspace.</Text> : null}
      {onAddPeople ? (
        <View className="mt-2">
          <Button label="Add people" variant="outline" size="sm" onPress={onAddPeople} />
        </View>
      ) : null}
    </View>
  )
}
