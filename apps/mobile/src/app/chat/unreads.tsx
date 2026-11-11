import { router } from 'expo-router'
import { FlatList, Pressable, View } from 'react-native'

import { useMessageText } from '../../components/chat/MessageList'
import { ChatPane, ConversationGlyph, MessagePreview } from '../../components/chat/Pane'
import { conversationTitle, inChannelGroup, type UnreadConversation, unreadGroups, unreadMessages } from '../../data/chat'
import * as chat from '../../data/chat-api'
import type { FollowedThreadRecord } from '../../data/chat-api'
import type { ConversationRecord, ConversationStateRecord } from '../../data/orbit'
import { useChatWrite, useMessages, useThread, useUnreadThreads } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useChatStates, useConversations, usePeople } from '../../queries'
import { Button, EmptyState, ErrorView, LoadingView, PullRefresh, Text } from '../../ui'

/**
 * The Unreads view: one group for each unread conversation and each thread with unread replies (followed or not), none
 * of a muted conversation, newest activity first.
 */
export default function UnreadsScreen() {
  const { user } = useOrbit()
  const conversations = useConversations()
  const states = useChatStates()
  const threads = useUnreadThreads()
  const run = useChatWrite()
  const groups = conversations.data && states.data && threads.data ? unreadGroups(conversations.data, states.data, threads.data, user.id) : []
  const refresh = () => Promise.all([conversations.refetch(), states.refetch(), threads.refetch()])

  return (
    <ChatPane
      title="Unreads"
      actions={groups.length > 0 ? <Button label="Mark all as read" variant="ghost" size="sm" muted onPress={() => void run('Could not mark everything as read', chat.markAllRead)} /> : undefined}
    >
      {conversations.isError || states.isError || threads.isError ? (
        <ErrorView error={conversations.error ?? states.error ?? threads.error} onRetry={() => void refresh()} />
      ) : !conversations.data || !states.data || !threads.data ? (
        <LoadingView />
      ) : groups.length === 0 ? (
        <EmptyState icon="checkCircle" title="You're all caught up" description="New messages and thread replies in your channels and direct messages show here." />
      ) : (
        // a virtualized list mounts only the groups near the screen, so a long list does not load every thread at once
        <FlatList
          data={groups}
          keyExtractor={(group) => (group.kind === 'thread' ? `thread-${group.thread.root.id}` : group.conversation.id)}
          refreshControl={<PullRefresh onRefresh={() => refresh()} />}
          renderItem={({ item: group }) =>
            group.kind === 'thread' ? (
              <ThreadGroup conversation={group.conversation} thread={group.thread} channel={group.channel} />
            ) : (
              <UnreadGroup conversation={group.conversation} state={group.state} />
            )
          }
        />
      )}
    </ChatPane>
  )
}

function UnreadGroup({ conversation, state }: UnreadConversation) {
  const { user } = useOrbit()
  const people = usePeople()
  const messages = useMessages(conversation.id, undefined)
  const words = useMessageText()
  const run = useChatWrite()
  const title = conversationTitle(conversation, people, user.id)
  const shown = unreadMessages(messages.data?.pages.flatMap((page) => page.items) ?? [], state.last_read_message_id, user.id)
  const more = Math.max(0, state.unread_count - shown.length)
  const open = () => router.push(`/chat/${conversation.id}`)

  return (
    <View accessibilityLabel={title} className="gap-3 px-3 py-3 border-b border-border">
      <View className="flex-row items-center gap-2">
        <Pressable accessibilityRole="link" accessibilityLabel={`Open ${title}`} onPress={open} className="shrink flex-row items-center gap-2">
          <ConversationGlyph conversation={conversation} title={title} userId={user.id} />
          <Text numberOfLines={1} className="font-noto-semibold text-[13px] leading-[20px] shrink">
            {title}
          </Text>
        </Pressable>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{state.unread_count} unread</Text>
        <View className="flex-1" />
        <Button label="Mark as read" variant="ghost" size="sm" muted onPress={() => void run('Could not mark as read', (client, workspaceId) => chat.markRead(client, workspaceId, conversation.id))} />
      </View>
      {messages.isError ? (
        <Text className="text-[13px] text-muted-foreground">Could not load these messages.</Text>
      ) : (
        shown.map((message) => <MessagePreview key={message.id} message={message} author={words.nameOf(message.author_id)} text={words.text(message.body)} onPress={open} flush />)
      )}
      {more > 0 && !messages.isPending ? (
        <Pressable accessibilityRole="link" onPress={open} className="self-start">
          <Text className="text-[12px] text-muted-foreground">
            {more} more unread {more === 1 ? 'message' : 'messages'}
          </Text>
        </Pressable>
      ) : null}
    </View>
  )
}

/**
 * One thread's unread replies, followed or not: the root for context, the newest few unread replies and "Mark as read".
 * A reply that the channel's group shows too is left out here.
 */
function ThreadGroup({ conversation, thread, channel }: { conversation: ConversationRecord; thread: FollowedThreadRecord; channel: ConversationStateRecord | null }) {
  const { user } = useOrbit()
  const people = usePeople()
  const replies = useThread(thread.root.id)
  const words = useMessageText()
  const run = useChatWrite()
  const title = conversationTitle(conversation, people, user.id)
  const unread = thread.state.unread_replies
  const items = (replies.data?.pages.flatMap((page) => page.items) ?? []).filter((message) => message.id !== thread.root.id)
  const all = unreadMessages(items, thread.state.last_read_reply_id, user.id, Infinity)
  const inChannel = all.filter((message) => inChannelGroup(message, channel)).length
  const shown = all.filter((message) => !inChannelGroup(message, channel)).slice(-5)
  const more = Math.max(0, unread - inChannel - shown.length)
  const open = () => router.push(`/chat/thread/${thread.root.id}`)

  return (
    <View accessibilityLabel={`Thread in ${title}`} className="gap-3 px-3 py-3 border-b border-border">
      <View className="flex-row items-center gap-2">
        <Pressable accessibilityRole="link" accessibilityLabel={`Open the thread in ${title}`} onPress={open} className="shrink flex-row items-center gap-2">
          <ConversationGlyph conversation={conversation} title={title} userId={user.id} />
          <Text numberOfLines={1} className="font-noto-semibold text-[13px] leading-[20px] shrink">
            Thread in {title}
          </Text>
        </Pressable>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">
          {unread} unread {unread === 1 ? 'reply' : 'replies'}
        </Text>
        <View className="flex-1" />
        <Button label="Mark as read" variant="ghost" size="sm" muted onPress={() => void run('Could not mark as read', (client, workspaceId) => chat.markThreadRead(client, workspaceId, thread.root.id))} />
      </View>
      <MessagePreview message={thread.root} author={words.nameOf(thread.root.author_id)} text={words.text(thread.root.body)} onPress={open} clamp={2} flush />
      {replies.isError ? (
        <Text className="text-[13px] text-muted-foreground">Could not load these replies.</Text>
      ) : (
        <View className="gap-3 pl-3 border-l-2 border-border">
          {shown.map((message) => (
            <MessagePreview key={message.id} message={message} author={words.nameOf(message.author_id)} text={words.text(message.body)} onPress={open} flush />
          ))}
        </View>
      )}
      {more > 0 && !replies.isPending ? (
        <Pressable accessibilityRole="link" onPress={open} className="self-start">
          <Text className="text-[12px] text-muted-foreground">
            {more} more unread {more === 1 ? 'reply' : 'replies'}
          </Text>
        </Pressable>
      ) : null}
    </View>
  )
}
