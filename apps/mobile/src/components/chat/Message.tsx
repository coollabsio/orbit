import { File, Paths } from 'expo-file-system'
import { useState } from 'react'
import { Image, Modal, Platform, Pressable, Share, useWindowDimensions, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { fileSize, hasReaction, isImage } from '../../data/chat'
import type { ChatFileRecord } from '../../data/chat-api'
import { relativeAgo, timeOfDay } from '../../data/format'
import type { MessageRecord } from '../../data/orbit'
import { showError, useServerImage } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { cssColors, useTheme } from '../../theme'
import { Avatar, Icon, IconButton, Spinner, Text } from '../../ui'
import { Emoji } from './EmojiPicker'

/** What a message row needs from the screen: names, the shown text of a body, and what a tap does. */
export interface MessageHandlers {
  nameOf: (userId: string) => string
  /** A stored body as shown: mentions as names, markdown as plain text. */
  text: (body: string) => string
  onLongPress: (message: MessageRecord) => void
  onReact: (message: MessageRecord, emoji: string) => void
  onAddReaction: (message: MessageRecord) => void
  /** Absent in a thread: its replies have no thread of their own. */
  onOpenThread?: (message: MessageRecord) => void
}

/**
 * One message at phone size, like the web: a 30px avatar, name and time when it starts a group, the text, files,
 * reactions and the thread summary. A long press opens the action sheet.
 */
export function MessageItem({ message, groupStart, highlighted = false, handlers }: { message: MessageRecord; groupStart: boolean; highlighted?: boolean; handlers: MessageHandlers }) {
  const { colors } = useTheme()
  const { user } = useOrbit()
  const author = handlers.nameOf(message.author_id)

  if (message.kind !== 'message') {
    const what = message.kind === 'join' ? 'joined' : message.kind === 'leave' ? 'left' : 'pinned a message'
    return (
      <Text className="text-[12px] leading-[16px] px-3 py-1 text-muted-foreground">
        {author} {what} · {timeOfDay(message.created_at)}
      </Text>
    )
  }

  const body = message.deleted ? 'Message deleted.' : handlers.text(message.body)
  return (
    <Pressable
      onLongPress={() => handlers.onLongPress(message)}
      delayLongPress={350}
      accessibilityHint="Long press for actions"
      className={`${highlighted ? 'bg-primary/12' : 'active:bg-muted/50'} ${groupStart ? 'mt-2' : 'mt-0'}`}
    >
      {message.reply_to ? <Quote author={handlers.nameOf(message.reply_to.author_id)} body={message.reply_to.sticker && !message.reply_to.body ? 'Sticker' : handlers.text(message.reply_to.body)} /> : null}
      <View className={`flex-row gap-2 px-3 pb-0.5 ${groupStart ? 'pt-1' : 'pt-0.5'}`}>
        <View className="w-7.5 pt-0.5">{groupStart ? <Avatar id={message.author_id} name={author} size={30} /> : null}</View>
        <View className="flex-1 min-w-0">
          {groupStart ? (
            <View className="flex-row items-baseline gap-2">
              <Text numberOfLines={1} className="font-noto-medium text-[14px] leading-[20px] shrink">
                {author}
              </Text>
              <Text className="text-[12px] leading-[16px] text-muted-foreground">{timeOfDay(message.created_at)}</Text>
              {message.pinned ? <Icon name="pinTack" size={12} color={colors.mutedForeground} /> : null}
            </View>
          ) : null}
          {message.forwarded ? (
            <View className="flex-row items-center gap-1">
              <Icon name="forward" size={12} color={colors.mutedForeground} />
              <Text className="text-[12px] leading-[16px] text-muted-foreground">Forwarded from {handlers.nameOf(message.forwarded.author_id)}</Text>
            </View>
          ) : null}
          {body ? (
            <Text className={`text-[14px] leading-[21px] ${message.deleted ? 'text-muted-foreground' : 'text-foreground/85'}`}>
              {body}
              {message.edited_at && !message.deleted ? <Text className="text-[11px] text-muted-foreground"> (edited)</Text> : null}
            </Text>
          ) : null}
          {message.sticker ? <Sticker url={message.sticker.url} name={message.sticker.name} /> : message.sticker_id && !message.deleted ? <Text className="text-[12px] text-muted-foreground">Sticker was deleted</Text> : null}
          {!message.deleted && message.attachments.length > 0 ? <Attachments files={message.attachments} hasText={body !== ''} /> : null}
          {!message.deleted && message.reactions.length > 0 ? (
            <View className="flex-row flex-wrap gap-1 mt-1">
              {message.reactions.map((reaction) => {
                const own = hasReaction(message, reaction.emoji, user.id)
                return (
                  <Pressable
                    key={reaction.emoji}
                    accessibilityRole="button"
                    accessibilityState={{ selected: own }}
                    accessibilityLabel={`${reaction.emoji} ${reaction.user_ids.length}, ${reaction.user_ids.map(handlers.nameOf).join(', ')} reacted`}
                    onPress={() => handlers.onReact(message, reaction.emoji)}
                    onLongPress={() => handlers.onLongPress(message)}
                    className={`h-6.5 min-w-11 flex-row items-center justify-center gap-1 px-2 rounded-full border ${own ? 'border-primary/40 bg-primary/10' : 'border-border bg-muted/50'} active:scale-97`}
                  >
                    <Emoji value={reaction.emoji} size={16} />
                    <Text className={`font-noto-medium text-[12px] leading-[16px] ${own ? 'text-foreground' : 'text-muted-foreground'}`}>{reaction.user_ids.length}</Text>
                  </Pressable>
                )
              })}
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Add reaction"
                onPress={() => handlers.onAddReaction(message)}
                className="h-6.5 w-8.5 items-center justify-center rounded-full border border-border active:bg-muted"
              >
                <Icon name="smile" size={14} color={colors.mutedForeground} />
              </Pressable>
            </View>
          ) : null}
          {message.reply_count > 0 && handlers.onOpenThread ? <ReplySummary message={message} handlers={handlers} /> : null}
        </View>
      </View>
    </Pressable>
  )
}

/** The web `MessageQuote`: a bent guide from the avatar column to the quoted author and the start of their text. */
function Quote({ author, body }: { author: string; body: string }) {
  return (
    <View className="flex-row items-center gap-2 px-3 pt-1">
      <View className="w-7.5 items-end self-end">
        <View className="w-[19px] h-2.5 -mr-1 rounded-tl-sm border-t-2 border-l-2 border-border" />
      </View>
      <Text numberOfLines={1} className="text-[12px] flex-1 leading-[20px] text-muted-foreground">
        <Text className="font-noto-medium text-[12px] text-foreground/90">{author} </Text>
        {body}
      </Text>
    </View>
  )
}

/** "2 replies · last reply 5m ago" with the last reply's preview, behind a 2px guide line. Opens the thread. */
function ReplySummary({ message, handlers }: { message: MessageRecord; handlers: MessageHandlers }) {
  const reply = message.last_reply
  const replies = `${message.reply_count} ${message.reply_count === 1 ? 'reply' : 'replies'}`
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`Open thread, ${replies}`}
      onPress={() => handlers.onOpenThread?.(message)}
      className="mt-1.5 -ml-1 py-1 pl-1 pr-2 rounded-md active:bg-muted"
    >
      <View className="border-l-2 border-border pl-2.5 gap-1">
        {reply ? (
          <View className="flex-row items-center gap-1.5 min-w-0">
            <Avatar id={reply.author_id} name={handlers.nameOf(reply.author_id)} size={16} />
            <Text numberOfLines={1} className="font-noto-medium text-[12px] leading-[16px] shrink-0 text-foreground/90">
              {handlers.nameOf(reply.author_id)}
            </Text>
            <Text numberOfLines={1} className="text-[13px] leading-[17px] shrink text-muted-foreground">
              {reply.sticker ? 'Sticker' : handlers.text(reply.body)}
            </Text>
          </View>
        ) : null}
        <View className="flex-row items-center gap-1.5">
          <Text className="font-noto-medium text-[12px] leading-[16px] text-foreground/90">{replies}</Text>
          {message.last_reply_at ? <Text className="text-[12px] leading-[16px] text-muted-foreground">last reply {relativeAgo(message.last_reply_at)}</Text> : null}
        </View>
      </View>
    </Pressable>
  )
}

function Sticker({ url, name }: { url: string; name: string }) {
  const image = useServerImage()
  return <Image accessibilityLabel={`Sticker ${name}`} source={image(url)} className="size-30 mt-1" resizeMode="contain" />
}

/** The web `Attachments` at phone size: images in a grid (one alone up to 300px high), other files as cards. */
export function Attachments({ files, hasText }: { files: readonly ChatFileRecord[]; hasText: boolean }) {
  const image = useServerImage()
  const { width: screen } = useWindowDimensions()
  const [viewing, setViewing] = useState<ChatFileRecord | null>(null)
  const images = files.filter((file) => isImage(file) && !file.deleted)
  const others = files.filter((file) => !isImage(file) || file.deleted)
  const maxWidth = Math.min(screen - 12 - 30 - 8 - 12, 512)
  const single = images.length === 1 ? images[0] : undefined
  const singleSize = single?.width && single.height ? fit(single.width, single.height, maxWidth, 300) : { width: maxWidth, height: 200 }
  const columns = images.length === 3 ? 3 : 2
  const cell = (maxWidth - (columns - 1) * 4) / columns

  return (
    <View className={`gap-2 ${hasText ? 'mt-2' : 'mt-1'}`}>
      {images.length > 0 ? (
        <View className="flex-row flex-wrap gap-1" style={{ maxWidth }}>
          {images.slice(0, 4).map((file, index) => (
            <Pressable key={file.id} accessibilityRole="imagebutton" accessibilityLabel={`Open image ${file.file_name}`} onPress={() => setViewing(file)}>
              <Image
                source={image(file.url)}
                className="rounded-md bg-muted" style={single ? singleSize : { width: cell, height: cell }}
                resizeMode={single ? 'contain' : 'cover'}
              />
              {index === 3 && images.length > 4 ? (
                <View className="absolute inset-0 rounded-md items-center justify-center bg-black/60">
                  <Text className="font-noto-semibold text-[18px] text-white">+{images.length - 4}</Text>
                </View>
              ) : null}
            </Pressable>
          ))}
        </View>
      ) : null}
      {others.map((file) => (
        <FileCard key={file.id} file={file} />
      ))}
      <ImageViewer file={viewing} onClose={() => setViewing(null)} />
    </View>
  )
}

const fit = (width: number, height: number, maxWidth: number, maxHeight: number) => {
  const scale = Math.min(1, maxWidth / width, maxHeight / height)
  return { width: Math.round(width * scale), height: Math.round(height * scale) }
}

/**
 * Downloads a chat file with the session's token into the cache and hands it to the system preview (Quick Look on
 * iOS, the app for its type on Android), where it can be saved or shared.
 */
function useOpenFile() {
  const { session } = useOrbit()
  const image = useServerImage()
  const [opening, setOpening] = useState<string | null>(null)
  const open = async (file: ChatFileRecord) => {
    if (Platform.OS === 'web') return void window.open(image(file.url).uri, '_blank')
    setOpening(file.id)
    try {
      const target = new File(Paths.cache, `chat-${file.id}-${file.file_name.replace(/[/\\]/g, '_')}`)
      const local = target.exists ? target : await File.downloadFileAsync(image(file.url).uri, target, { headers: { Authorization: `Bearer ${session.token}` }, idempotent: true })
      if (await local.canPreview()) await local.preview()
      else await Share.share({ url: local.uri, title: file.file_name })
    } catch (error) {
      showError(`Could not open ${file.file_name}`, error)
    } finally {
      setOpening(null)
    }
  }
  return { open, opening }
}

function FileCard({ file }: { file: ChatFileRecord }) {
  const { colors } = useTheme()
  const { open, opening } = useOpenFile()
  if (file.deleted) {
    return (
      <View accessibilityLabel={`Deleted file ${file.file_name}`} className="max-w-112 flex-row items-center gap-3 p-3 rounded-lg border border-dashed border-border">
        <Icon name="attach" size={28} color={colors.mutedForeground} />
        <View className="flex-1 min-w-0">
          <Text className="font-noto-medium text-[14px] leading-[20px] text-muted-foreground">Deleted file</Text>
          <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">{file.file_name} · an admin deleted it</Text>
        </View>
      </View>
    )
  }
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`Open ${file.file_name}, ${fileSize(file.size_bytes)}`}
      onPress={() => void open(file)}
      className="max-w-112 flex-row items-center gap-3 p-3 rounded-lg border border-border bg-background active:bg-muted"
    >
      <Icon name="attach" size={28} color={colors.mutedForeground} />
      <View className="flex-1 min-w-0">
        <Text numberOfLines={1} className="font-noto-medium text-[14px] leading-[20px]">
          {file.file_name}
        </Text>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{fileSize(file.size_bytes)}</Text>
      </View>
      {opening === file.id ? <Spinner /> : <Icon name="download" size={20} color={colors.mutedForeground} />}
    </Pressable>
  )
}

/** An image of a message over the whole screen, with its name, Close and Open (save or share). */
function ImageViewer({ file, onClose }: { file: ChatFileRecord | null; onClose: () => void }) {
  const image = useServerImage()
  const { open, opening } = useOpenFile()
  // Read outside the Modal: a SafeAreaView inside a full-screen iOS Modal measures no insets, and the bar sat under the status bar.
  const insets = useSafeAreaInsets()
  return (
    <Modal visible={file !== null} animationType="fade" onRequestClose={onClose} supportedOrientations={['portrait', 'landscape']}>
      <View className="flex-1 bg-black" style={{ paddingTop: insets.top, paddingBottom: insets.bottom, paddingLeft: insets.left, paddingRight: insets.right }}>
        <View className="h-12 flex-row items-center gap-2 px-2">
          <IconButton icon="x" label="Close" size={36} iconSize={22} color={cssColors.white} onPress={onClose} />
          <Text numberOfLines={1} className="font-noto-medium text-[14px] flex-1 text-white">
            {file?.file_name}
          </Text>
          {file ? opening ? <Spinner color={cssColors.white} /> : <IconButton icon="download" label="Open or save" size={36} iconSize={22} color={cssColors.white} onPress={() => void open(file)} /> : null}
        </View>
        {file ? <Image accessibilityLabel={file.file_name} source={image(file.url)} className="flex-1" resizeMode="contain" /> : null}
      </View>
    </Modal>
  )
}
