import { router } from 'expo-router'
import { type ConversationList } from '../../chat-cache'
import { FlatList, RefreshControl } from 'react-native'

import { conversationTitle } from '../../data/chat'
import type { MessageRecord } from '../../data/orbit'
import { useConversationList } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useConversations, usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { EmptyState, ErrorView, type IconName, LoadingView } from '../../ui'
import { useMessageText } from './MessageList'
import { ChatPane, MessagePreview } from './Pane'

const LOOK: Record<ConversationList, { title: string; icon: IconName; empty: string; description: string }> = {
  threads: { title: 'Threads', icon: 'layer', empty: 'No threads yet', description: 'Reply in a thread under any message, and it shows here.' },
  pins: { title: 'Pinned messages', icon: 'pinTack', empty: 'No pinned messages', description: 'Pin a message from its actions to keep it here.' },
  files: { title: 'Files', icon: 'attach', empty: 'No files yet', description: 'Files sent in this conversation show here.' },
}

/** The web's Threads, Pinned messages and Files side panes as screens. A message opens where it is; a thread root its thread. */
export function ConversationListScreen({ conversationId, kind }: { conversationId: string; kind: ConversationList }) {
  const { user } = useOrbit()
  const { colors } = useTheme()
  const people = usePeople()
  const conversation = useConversations().data?.find((item) => item.id === conversationId)
  const list = useConversationList(conversationId, kind)
  const words = useMessageText()
  const look = LOOK[kind]
  const open = (message: MessageRecord) =>
    kind === 'threads' ? router.push(`/chat/thread/${message.id}`) : router.dismissTo(`/chat/${conversationId}?message=${message.id}`)

  return (
    <ChatPane title={look.title} subtitle={conversation ? (conversation.kind === 'dm' ? conversationTitle(conversation, people, user.id) : `#${conversation.name}`) : undefined}>
      {list.isError ? (
        <ErrorView error={list.error} onRetry={() => void list.refetch()} />
      ) : !list.data ? (
        <LoadingView />
      ) : list.data.length === 0 ? (
        <EmptyState icon={look.icon} title={look.empty} description={look.description} />
      ) : (
        <FlatList
          data={list.data}
          keyExtractor={(message) => message.id}
          contentContainerClassName="p-1"
          refreshControl={<RefreshControl refreshing={list.isRefetching} onRefresh={() => void list.refetch()} tintColor={colors.mutedForeground} />}
          renderItem={({ item }) => (
            <MessagePreview
              message={item}
              author={words.nameOf(item.author_id)}
              text={words.text(item.body)}
              context={kind === 'threads' ? `${item.reply_count} ${item.reply_count === 1 ? 'reply' : 'replies'}` : undefined}
              onPress={() => open(item)}
            />
          )}
        />
      )}
    </ChatPane>
  )
}
