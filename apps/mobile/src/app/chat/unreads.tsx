import { router } from 'expo-router'
import { Pressable, RefreshControl, ScrollView, View } from 'react-native'

import { useMessageText } from '../../components/chat/MessageList'
import { ChatPane, ConversationGlyph, MessagePreview } from '../../components/chat/Pane'
import { conversationTitle, type UnreadConversation, unreadConversations, unreadMessages } from '../../data/chat'
import * as chat from '../../data/chat-api'
import { useChatWrite, useMessages } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useChatStates, useConversations, usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { Button, EmptyState, ErrorView, LoadingView, Text } from '../../ui'
import { ChatColorScope } from '../../components/chat/ChatColorScope'

/** The Unreads view: one group for each unread conversation that is not muted, newest activity first. */
export default function UnreadsScreen() {
  return (
    <ChatColorScope surface="page">
      <UnreadsScreenContent />
    </ChatColorScope>
  )
}

function UnreadsScreenContent() {
  const { user } = useOrbit()
  const { colors } = useTheme()
  const conversations = useConversations()
  const states = useChatStates()
  const run = useChatWrite()
  const groups = conversations.data && states.data ? unreadConversations(conversations.data, states.data, user.id) : []
  const refresh = () => Promise.all([conversations.refetch(), states.refetch()])

  return (
    <ChatPane
      title="Unreads"
      actions={groups.length > 0 ? <Button label="Mark all as read" variant="ghost" size="sm" onPress={() => void run('Could not mark everything as read', chat.markAllRead)} /> : undefined}
    >
      {conversations.isError || states.isError ? (
        <ErrorView error={conversations.error ?? states.error} onRetry={() => void refresh()} />
      ) : !conversations.data || !states.data ? (
        <LoadingView />
      ) : groups.length === 0 ? (
        <EmptyState icon="checkCircle" title="You're all caught up" description="New messages in your channels and direct messages show here." />
      ) : (
        <ScrollView refreshControl={<RefreshControl refreshing={states.isRefetching} onRefresh={() => void refresh()} tintColor={colors.mutedForeground} />}>
          {groups.map((group) => (
            <UnreadGroup key={group.conversation.id} {...group} />
          ))}
        </ScrollView>
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
  const shown = unreadMessages(messages.data?.pages.flatMap((page) => page.items) ?? [], state, user.id)
  const more = Math.max(0, state.unread_count - shown.length)
  const open = () => router.push(`/chat/${conversation.id}`)

  return (
    <View accessibilityLabel={title} className="gap-1 px-2 py-3 border-b border-border">
      <View className="flex-row items-center gap-2 px-1">
        <Pressable accessibilityRole="link" accessibilityLabel={`Open ${title}`} onPress={open} className="shrink flex-row items-center gap-2">
          <ConversationGlyph conversation={conversation} title={title} userId={user.id} />
          <Text numberOfLines={1} className="font-noto-semibold text-[13px] shrink">
            {title}
          </Text>
        </Pressable>
        <Text className="text-[12px] text-muted-foreground">{state.unread_count} unread</Text>
        <View className="flex-1" />
        <Button label="Mark as read" variant="ghost" size="sm" onPress={() => void run('Could not mark as read', (client, workspaceId) => chat.markRead(client, workspaceId, conversation.id))} />
      </View>
      {messages.isError ? (
        <Text className="text-[13px] px-1 text-muted-foreground">Could not load these messages.</Text>
      ) : (
        shown.map((message) => <MessagePreview key={message.id} message={message} author={words.nameOf(message.author_id)} text={words.text(message.body)} onPress={open} />)
      )}
      {more > 0 && !messages.isPending ? (
        <Pressable accessibilityRole="link" onPress={open} className="px-1">
          <Text className="text-[12px] text-muted-foreground">
            {more} more unread {more === 1 ? 'message' : 'messages'}
          </Text>
        </Pressable>
      ) : null}
    </View>
  )
}
