import { useQueryClient } from '@tanstack/react-query'
import { applyChatEvents } from '../../../chat-cache'
import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useMemo, useRef, useState } from 'react'
import { KeyboardAvoidingView, Platform, Pressable, Share, View } from 'react-native'

import { Composer, useComposer } from '../../../components/chat/Composer'
import { MessageItem } from '../../../components/chat/Message'
import { type ListRow, MessageList, useMessageSheet } from '../../../components/chat/MessageList'
import { ActionSheet } from '../../../components/chat/Sheet'
import { buildMessageRows, conversationTitle, firstLine } from '../../../data/chat'
import * as chat from '../../../data/chat-api'
import { useChatWrite, useFollowedThreads, useThread, useThreadFollowsRoot } from '../../../hooks/chat'
import { useOrbit } from '../../../orbit-context'
import { useConversations, usePeople } from '../../../queries'
import { useTheme } from '../../../theme'
import { ErrorView, Icon, IconButton, LoadingView, SafeAreaView, Text } from '../../../ui'

/**
 * A thread in full view, as the web's `/chat/:id/thread/:messageId` on a phone: the root, its replies, and a composer
 * that replies in the thread (and can also send to the conversation). The bell follows or unfollows it.
 */
export default function ThreadScreen() {
  const { id: rootId } = useLocalSearchParams<{ id: string }>()
  const { client, workspaceId, user, session } = useOrbit()
  const [actions, setActions] = useState(false)
  const queryClient = useQueryClient()
  const { colors } = useTheme()
  const people = usePeople()
  const conversations = useConversations()
  const followed = useFollowedThreads()
  const run = useChatWrite()
  const thread = useThread(rootId)

  const root = thread.data?.pages.at(-1)?.root
  const conversationId = root?.conversation_id ?? ''
  const conversation = conversations.data?.find((item) => item.id === conversationId)
  const replies = useMemo(() => thread.data?.pages.flatMap((page) => page.items) ?? [], [thread.data])
  const composer = useComposer(conversationId, rootId)
  const { sheet, handlers } = useMessageSheet({ inThread: true, composer, loaded: replies })
  useThreadFollowsRoot(root)

  const state = followed.data?.find((item) => item.root.id === rootId)?.state ?? thread.data?.pages.at(-1)?.state ?? null
  const following = state?.following ?? false
  const unread = state?.unread_replies ?? 0
  const newestId = replies.at(-1)?.id
  // The web's "New" line: the read cursor when the thread opened stays while it is open.
  const opened = useRef<{ cursor: string | null | undefined } | null>(null)
  if (!opened.current && state) opened.current = { cursor: state.unread_replies > 0 ? state.last_read_reply_id : undefined }
  const cursor = opened.current?.cursor
  useEffect(() => {
    if (!root || unread === 0) return
    chat.markThreadRead(client, workspaceId, rootId).then(
      ({ events }) => applyChatEvents(queryClient, workspaceId, events),
      () => undefined,
    )
  }, [client, newestId, queryClient, root, rootId, unread, workspaceId])

  const rows: ListRow[] = useMemo(
    () => [
      ...composer.pending
        .filter((item) => !replies.some((message) => message.nonce === item.nonce))
        .map((item): ListRow => ({ type: 'pending', key: item.nonce, pending: item }))
        .reverse(),
      ...buildMessageRows(replies, { lastReadMessageId: cursor, userId: user.id, sameDayAs: root?.created_at }).reverse(),
    ],
    [replies, composer.pending, cursor, user.id, root?.created_at],
  )
  const title = conversation ? conversationTitle(conversation, people, user.id) : ''
  const where = conversation ? (conversation.kind === 'dm' ? title : `#${title}`) : ''
  const member = conversation?.member_ids.includes(user.id) ?? false
  const rootLine = root && !root.deleted ? firstLine(handlers.text(root.body), 60) : ''
  const close = () => (router.canGoBack() ? router.back() : router.navigate(conversationId ? `/chat/${conversationId}` : '/chat'))
  const showInConversation = () => router.push(`/chat/${conversationId}?message=${rootId}`)
  const copyLink = () => {
    const link = `${session.server}/chat/${conversationId}/thread/${rootId}`
    if (Platform.OS === 'web') void navigator.clipboard.writeText(link)
    else void Share.share({ message: link })
  }

  return (
    <SafeAreaView edges={['top', 'left', 'right', 'bottom']} className="flex-1 bg-card">
      {/* the web's full-view `ThreadHeader`: Back, "#channel › Thread · “first line”", follow and `…` */}
      <View className="h-12 flex-row items-center gap-1 px-3">
        {/* unlike the conversation header, the thread's Back and `…` are not muted on the web */}
        <IconButton icon="back" label="Back" size={32} iconSize={20} color={colors.foreground} onPress={close} />
        <View className="flex-1 min-w-0 flex-row items-center gap-1.5">
          {where ? (
            <Pressable accessibilityRole="link" accessibilityLabel={`Show in ${where}`} onPress={showInConversation} className="shrink-0">
              <Text numberOfLines={1} className="font-noto-semibold text-[13px] leading-[19px]">
                {where}
              </Text>
            </Pressable>
          ) : null}
          {/* a long first line must not squeeze the separator */}
          {where ? (
            <View className="shrink-0">
              <Icon name="chevronRight" size={14} color={colors.mutedForeground} />
            </View>
          ) : null}
          <Text accessibilityRole="header" numberOfLines={1} className="font-noto-semibold text-[13px] leading-[19px] shrink">
            Thread
            {rootLine ? <Text className="font-noto text-[13px] text-muted-foreground"> · “{rootLine}”</Text> : null}
          </Text>
        </View>
        <IconButton
          icon={following ? 'bell' : 'bellOff'}
          label={following ? 'Unfollow thread' : 'Follow thread'}
          size={32}
          iconSize={20}
          color={following ? colors.primary : colors.mutedForeground}
          disabled={!root}
          onPress={() => void run(following ? 'Could not unfollow the thread' : 'Could not follow the thread', (client, workspaceId) => chat.followThread(client, workspaceId, rootId, !following))}
        />
        <IconButton icon="more" label="Thread actions" size={32} iconSize={20} color={colors.foreground} disabled={!conversationId} onPress={() => setActions(true)} />
      </View>
      <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        {!thread.data || !root ? (
          thread.isError ? (
            <ErrorView error={thread.error} onRetry={() => void thread.refetch()} />
          ) : (
            <LoadingView />
          )
        ) : (
          <MessageList
            rows={rows}
            handlers={handlers}
            fromTop
            loadingOlder={thread.isFetchingPreviousPage}
            onOlder={() => {
              if (thread.hasPreviousPage && !thread.isFetchingPreviousPage) void thread.fetchPreviousPage()
            }}
            start={
              thread.hasPreviousPage ? undefined : (
                <View>
                  <MessageItem message={root} groupStart handlers={handlers} />
                  <View accessibilityRole="header" className="flex-row items-center gap-3 px-4 py-2">
                    <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground">
                      {root.reply_count} {root.reply_count === 1 ? 'reply' : 'replies'}
                    </Text>
                    <View className="flex-1 h-px bg-border" />
                  </View>
                </View>
              )
            }
          />
        )}
        {root && member && !conversation?.archived ? <Composer state={composer} placeholder="Reply…" nameOf={handlers.nameOf} alsoSendTo={conversation?.kind === 'dm' ? 'this conversation' : where} /> : null}
      </KeyboardAvoidingView>
      {sheet}
      <ActionSheet
        visible={actions}
        onClose={() => setActions(false)}
        items={[
          { key: 'copy', label: Platform.OS === 'web' ? 'Copy link' : 'Copy or share link', icon: 'copy', onPress: copyLink },
          { key: 'show', label: `Show in ${where}`, icon: 'chat', onPress: showInConversation },
          { key: 'close', label: 'Close thread', icon: 'x', onPress: close },
        ]}
      />
    </SafeAreaView>
  )
}
