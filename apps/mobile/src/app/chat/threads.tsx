import { router } from 'expo-router'
import { Pressable, RefreshControl, ScrollView, View } from 'react-native'

import { useMessageText } from '../../components/chat/MessageList'
import { ChatPane, ConversationGlyph, MessagePreview } from '../../components/chat/Pane'
import { conversationTitle, sortFollowedThreads } from '../../data/chat'
import { relativeAgo } from '../../data/format'
import { useFollowedThreads } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useConversations, usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { CountBadge, EmptyState, ErrorView, LoadingView, Text } from '../../ui'
import { ChatColorScope } from '../../components/chat/ChatColorScope'

/** The Threads view: the threads you follow, unread first, then by last reply. A row opens the thread. */
export default function ThreadsScreen() {
  return (
    <ChatColorScope surface="page">
      <ThreadsScreenContent />
    </ChatColorScope>
  )
}

function ThreadsScreenContent() {
  const { user } = useOrbit()
  const { colors } = useTheme()
  const threads = useFollowedThreads()
  const conversations = useConversations().data ?? []
  const people = usePeople()
  const words = useMessageText()
  const rows = sortFollowedThreads(threads.data ?? [])

  return (
    <ChatPane title="Threads">
      {threads.isError ? (
        <ErrorView error={threads.error} onRetry={() => void threads.refetch()} />
      ) : !threads.data ? (
        <LoadingView />
      ) : rows.length === 0 ? (
        <EmptyState icon="layer" title="Threads you follow appear here" description="Reply to a message or follow its thread, and new replies show up in this list." />
      ) : (
        <ScrollView refreshControl={<RefreshControl refreshing={threads.isRefetching} onRefresh={() => void threads.refetch()} tintColor={colors.mutedForeground} />}>
          {rows.map((thread) => {
            const conversation = conversations.find((item) => item.id === thread.conversation_id)
            const title = conversation ? conversationTitle(conversation, people, user.id) : 'Unknown conversation'
            const unread = thread.state.unread_replies
            const replies = `${thread.root.reply_count} ${thread.root.reply_count === 1 ? 'reply' : 'replies'}`
            const lastReplyAt = thread.last_reply?.created_at ?? thread.root.last_reply_at
            return (
              <Pressable
                key={thread.root.id}
                accessibilityRole="link"
                accessibilityLabel={`Thread in ${title}, ${replies}${unread > 0 ? `, ${unread} unread` : ''}`}
                onPress={() => router.push(`/chat/thread/${thread.root.id}`)}
                className="gap-1 px-2 py-3 border-b border-border active:bg-muted/40"
              >
                <View className="flex-row items-center gap-1.5 px-1">
                  {conversation ? <ConversationGlyph conversation={conversation} title={title} userId={user.id} /> : null}
                  <Text numberOfLines={1} className={`text-[13px] shrink ${unread > 0 ? 'font-noto-semibold' : 'font-noto-medium'}`}>
                    {title}
                  </Text>
                  <Text className="text-[12px] text-muted-foreground">{replies}</Text>
                  {lastReplyAt ? <Text className="text-[12px] text-muted-foreground">last reply {relativeAgo(lastReplyAt)}</Text> : null}
                  <View className="flex-1" />
                  {unread > 0 ? <CountBadge count={unread} /> : null}
                </View>
                <MessagePreview message={thread.root} author={words.nameOf(thread.root.author_id)} text={words.text(thread.root.body)} clamp={2} />
                {thread.last_reply ? (
                  <View className="ml-3 border-l-2 border-border">
                    <MessagePreview message={thread.last_reply} author={words.nameOf(thread.last_reply.author_id)} text={words.text(thread.last_reply.body)} clamp={2} />
                  </View>
                ) : null}
              </Pressable>
            )
          })}
        </ScrollView>
      )}
    </ChatPane>
  )
}
