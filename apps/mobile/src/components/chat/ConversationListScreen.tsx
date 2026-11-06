import { router } from 'expo-router'
import { type ConversationList } from '../../chat-cache'
import { useState } from 'react'
import { FlatList, Image, Pressable, ScrollView, useWindowDimensions, View } from 'react-native'

import { fileSize, isImage } from '../../data/chat'
import type { ChatFileRecord } from '../../data/chat-api'
import * as chat from '../../data/chat-api'
import { relativeAgo } from '../../data/format'
import type { MessageRecord } from '../../data/orbit'
import { useChatWrite, useConversationList, useServerImage } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useConversations, usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { Button, EmptyState, ErrorView, Icon, type IconName, IconButton, LoadingView, PullRefresh, Spinner, Text } from '../../ui'
import { ImageViewer, useOpenFile } from './Message'
import { useMessageText } from './MessageList'
import { AvatarStack, ChatSidePane, MessageByline } from './Pane'

const LOOK: Record<ConversationList, { title: string; icon: IconName; empty: string; description: string }> = {
  threads: { title: 'Threads', icon: 'messages2', empty: 'No threads yet', description: 'Reply to a message to start one.' },
  pins: { title: 'Pinned messages', icon: 'pinTack', empty: 'No pinned messages', description: 'Pin a message to keep it here.' },
  // the web says "Drop a file", which a phone cannot do
  files: { title: 'Files', icon: 'folder', empty: 'No files yet', description: 'Attach a file in the conversation to share it.' },
}

/** The web's Threads, Pinned messages and Files side panes as screens. Jump opens the message where it is. */
export function ConversationListScreen({ conversationId, kind }: { conversationId: string; kind: ConversationList }) {
  const list = useConversationList(conversationId, kind)
  const look = LOOK[kind]

  return (
    <ChatSidePane title={look.title}>
      {list.isError ? (
        <ErrorView error={list.error} onRetry={() => void list.refetch()} />
      ) : !list.data ? (
        <LoadingView />
      ) : list.data.length === 0 || (kind === 'files' && list.data.every((message) => message.attachments.length === 0)) ? (
        <EmptyState icon={look.icon} title={look.empty} description={look.description} />
      ) : kind === 'files' ? (
        <Files conversationId={conversationId} messages={list.data} onRefresh={() => list.refetch()} />
      ) : (
        <FlatList
          data={kind === 'threads' ? [...list.data].sort((a, b) => Date.parse(b.last_reply_at ?? b.created_at) - Date.parse(a.last_reply_at ?? a.created_at)) : list.data}
          keyExtractor={(message) => message.id}
          contentContainerClassName="p-2 gap-1"
          refreshControl={<PullRefresh onRefresh={() => list.refetch()} />}
          renderItem={({ item }) => (kind === 'threads' ? <ThreadRow root={item} /> : <PinRow message={item} conversationId={conversationId} onUnpinned={() => void list.refetch()} />)}
        />
      )}
    </ChatSidePane>
  )
}

const jumpTo = (conversationId: string, message: MessageRecord) => router.dismissTo(`/chat/${conversationId}?message=${message.id}`)

/** The body as the web `MessageBody` shows it in a pane, flattened to text. */
function Body({ message, maxHeight }: { message: MessageRecord; maxHeight?: number }) {
  const words = useMessageText()
  if (message.deleted) return <Text className="text-[14px] leading-[21px] italic text-muted-foreground">Message deleted.</Text>
  const text = words.text(message.body)
  return text ? (
    <View className="overflow-hidden" style={maxHeight ? { maxHeight } : undefined}>
      <Text className="text-[14px] leading-[21px] text-foreground/85">{text}</Text>
    </View>
  ) : null
}

/** A pinned message: byline, body, then Jump and (for a member of a live conversation) Unpin. */
function PinRow({ message, conversationId, onUnpinned }: { message: MessageRecord; conversationId: string; onUnpinned: () => void }) {
  const { user } = useOrbit()
  const words = useMessageText()
  const run = useChatWrite()
  const conversation = useConversations().data?.find((item) => item.id === conversationId)
  // a reader of a public channel who has not joined can look, but not change the pins
  const canUnpin = Boolean(conversation?.member_ids.includes(user.id) && !conversation.archived)
  const unpin = async () => {
    if (await run('Could not unpin the message', (client, workspaceId) => chat.setPinned(client, workspaceId, message.id, false))) onUnpinned()
  }
  return (
    <View className="gap-1 rounded-lg p-2">
      <MessageByline message={message} author={words.nameOf(message.author_id)} />
      <Body message={message} />
      <View className="-ml-2 flex-row items-center gap-1">
        <Button label="Jump" variant="ghost" size="xs" onPress={() => jumpTo(conversationId, message)} />
        {canUnpin ? <Button label="Unpin" variant="ghost" size="xs" onPress={() => void unpin()} /> : null}
      </View>
    </View>
  )
}

/** A thread of the conversation: its root (the first lines), then who replied, how many and when, which opens it. */
function ThreadRow({ root }: { root: MessageRecord }) {
  const words = useMessageText()
  const people = usePeople()
  const repliers = root.reply_user_ids.slice(0, 3).flatMap((id) => people.find((person) => person.id === id) ?? [])
  const replies = root.reply_count === 1 ? '1 reply' : `${root.reply_count} replies`
  return (
    <View className="gap-1 rounded-lg p-2">
      <MessageByline message={root} author={words.nameOf(root.author_id)} />
      {/* the web's `max-h-28` */}
      <Body message={root} maxHeight={112} />
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Open thread, ${replies}`}
        onPress={() => router.push(`/chat/thread/${root.id}`)}
        className="-ml-2 h-7 max-w-full flex-row items-center gap-2 self-start rounded-md px-2.5 active:bg-muted"
      >
        {repliers.length > 0 ? <AvatarStack people={repliers} size={18} /> : null}
        <Text className="font-noto-medium text-[12.8px] leading-[18px]">{replies}</Text>
        {root.last_reply_at ? (
          <Text numberOfLines={1} className="shrink text-[12.8px] leading-[18px] text-muted-foreground">
            Last reply {relativeAgo(root.last_reply_at)}
          </Text>
        ) : null}
      </Pressable>
    </View>
  )
}

interface SharedFile {
  file: ChatFileRecord
  message: MessageRecord
}

/** The web `FilesPane`: images as a 3-column grid, other files as rows, newest first. */
function Files({ conversationId, messages, onRefresh }: { conversationId: string; messages: readonly MessageRecord[]; onRefresh: () => Promise<unknown> }) {
  const { colors } = useTheme()
  const words = useMessageText()
  const image = useServerImage()
  const { open, opening } = useOpenFile()
  const { width } = useWindowDimensions()
  const [viewing, setViewing] = useState<ChatFileRecord | null>(null)
  const shared: SharedFile[] = messages.flatMap((message) => message.attachments.map((file) => ({ file, message })))
  const images = shared.filter(({ file }) => isImage(file) && !file.deleted)
  const others = shared.filter(({ file }) => !isImage(file) || file.deleted)
  // the body's 8px and the grid's 8px on each side, 4px between three cells
  const cell = Math.floor((width - 32 - 8) / 3)

  return (
    <ScrollView contentContainerClassName="p-2 gap-3" refreshControl={<PullRefresh onRefresh={onRefresh} />}>
      {images.length > 0 ? (
        <View accessibilityLabel="Images">
          <Text className="px-2 py-1 text-[12px] leading-[16px] text-muted-foreground">Images</Text>
          <View className="flex-row flex-wrap gap-1 px-2">
            {images.map(({ file, message }) => (
              <View key={file.id} className="overflow-hidden rounded-md border border-border" style={{ width: cell, height: cell }}>
                <Pressable accessibilityRole="imagebutton" accessibilityLabel={`Open image ${file.file_name}`} onPress={() => setViewing(file)} className="flex-1">
                  <Image source={image(file.url)} className="flex-1" resizeMode="cover" />
                </Pressable>
                <View className="absolute right-1 bottom-1 rounded-md bg-secondary">
                  <IconButton icon="chat" label={`Go to the message with ${file.file_name}`} size={24} iconSize={12} color={colors.secondaryForeground} onPress={() => jumpTo(conversationId, message)} />
                </View>
              </View>
            ))}
          </View>
        </View>
      ) : null}
      {others.length > 0 ? (
        <View accessibilityLabel="Other files">
          <Text className="px-2 py-1 text-[12px] leading-[16px] text-muted-foreground">Files</Text>
          {others.map(({ file, message }) => (
            <View key={file.id} className="flex-row items-center gap-2 rounded-lg px-2 py-1.5">
              <Icon name="docs" size={20} color={colors.mutedForeground} />
              <View className="flex-1 min-w-0">
                <Text numberOfLines={1} className="font-noto-medium text-[14px] leading-[20px]">
                  {file.deleted ? 'Deleted file' : file.file_name}
                </Text>
                <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">
                  {file.deleted ? file.file_name : fileSize(file.size_bytes)} · {words.nameOf(message.author_id)} · {relativeAgo(message.created_at)}
                </Text>
              </View>
              <IconButton icon="chat" label={`Go to the message with ${file.file_name}`} size={28} iconSize={16} color={colors.foreground} onPress={() => jumpTo(conversationId, message)} />
              {file.deleted ? null : opening === file.id ? (
                <View className="size-7 items-center justify-center">
                  <Spinner />
                </View>
              ) : (
                <IconButton icon="download" label={`Download ${file.file_name}`} size={28} iconSize={16} color={colors.foreground} onPress={() => void open(file)} />
              )}
            </View>
          ))}
        </View>
      ) : null}
      <ImageViewer file={viewing} onClose={() => setViewing(null)} />
    </ScrollView>
  )
}
