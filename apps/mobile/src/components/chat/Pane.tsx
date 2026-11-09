import { router } from 'expo-router'
import type { ReactNode } from 'react'
import { Pressable, View } from 'react-native'

import type { TextSegment } from '../../data/chat'
import { presenceOf, type ShownPresence, usePresence } from '../../data/chat-presence'
import { relativeAgo, shortDate, timeOfDay } from '../../data/format'
import type { ConversationRecord, MessageRecord } from '../../data/orbit'
import { currentStatus } from '../../data/status'
import { useOrbit } from '../../orbit-context'
import { useTheme } from '../../theme'
import { Avatar, HeaderTitle, Icon, IconButton, SafeAreaView, Text } from '../../ui'

/** A chat screen with the web's 48px pane header: Back, the title, and actions at the end. */
export function ChatPane({ title, subtitle, actions, children }: { title: string; subtitle?: string; actions?: ReactNode; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-card">
      <View className="h-12 flex-row items-center gap-1 px-2">
        <IconButton icon="back" label="Back" size={32} iconSize={20} color={colors.mutedForeground} onPress={back} />
        <View className="flex-1 min-w-0">
          <Text accessibilityRole="header" numberOfLines={1} className="font-noto-semibold text-[14px] leading-[18px]">
            {title}
          </Text>
          {subtitle ? (
            <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">
              {subtitle}
            </Text>
          ) : null}
        </View>
        {actions}
      </View>
      {children}
    </SafeAreaView>
  )
}

const back = () => (router.canGoBack() ? router.back() : router.navigate('/chat'))

/**
 * The web `RightPane` as a screen (on a phone the web shows it as a sheet over the conversation): the 48px header with
 * the title, actions, and Close at the end.
 */
export function ChatSidePane({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-card">
      <View className="min-h-12 flex-row items-center gap-2 px-3 py-1.5">
        <View accessibilityRole="header" className="shrink">
          <HeaderTitle>{title}</HeaderTitle>
        </View>
        <View className="ml-auto flex-row items-center gap-2">
          {actions}
          <IconButton icon="close" label={`Close ${title.toLowerCase()}`} size={32} iconSize={20} color={colors.foreground} onPress={back} />
        </View>
      </View>
      {children}
    </SafeAreaView>
  )
}

/**
 * How a member shows to the others, from the live socket. The user's own status is known without it (the web gets it
 * back from the server); invisible shows as offline, as the others see it.
 */
export function useShownPresence(): (userId: string) => ShownPresence {
  const map = usePresence()
  const { user } = useOrbit()
  const own = currentStatus(user.status).presence
  return (userId) => (userId === user.id ? (own === 'invisible' ? 'offline' : own) : presenceOf(map, userId).status)
}

/** The web `MessageByline`: 20px avatar, the author's name, the relative time, then `children` (the conversation of a search hit). */
export function MessageByline({ message, author, children }: { message: Pick<MessageRecord, 'author_id' | 'created_at'>; author: string; children?: ReactNode }) {
  return (
    <View className="flex-row items-center gap-1.5 min-w-0">
      <Avatar id={message.author_id} name={author} size={20} />
      <Text numberOfLines={1} className="font-noto-medium text-[13px] leading-[18px] shrink">
        {author}
      </Text>
      <Text className="text-[12px] leading-[16px] text-muted-foreground">{relativeAgo(message.created_at)}</Text>
      {children}
    </View>
  )
}

/** The web `UserAvatarStack`: up to `max` avatars overlapping by 8px, each with a 1px ring of the card. */
export function AvatarStack({ people, size = 18, max = 3 }: { people: readonly { id: string; name: string }[]; size?: number; max?: number }) {
  const shown = people.slice(0, max)
  const rest = people.length - shown.length
  return (
    <View className="flex-row">
      {shown.map((person, index) => (
        <View key={person.id} className={`rounded-full border border-card ${index > 0 ? '-ml-2' : ''}`}>
          <Avatar id={person.id} name={person.name} size={size} />
        </View>
      ))}
      {rest > 0 ? (
        <View className="-ml-2 items-center justify-center rounded-full border border-card bg-muted" style={{ width: size + 2, height: size + 2 }}>
          <Text className="font-noto-semibold text-muted-foreground" style={{ fontSize: Math.max(9, Math.round(size * 0.38)) }}>
            +{rest}
          </Text>
        </View>
      ) : null}
    </View>
  )
}

/** The web `ConversationIcon`: `#` or a lock at 90% of `size`, or the other person's avatar with presence for a DM. */
export function ConversationGlyph({ conversation, title, userId, size = 18 }: { conversation: ConversationRecord; title: string; userId: string; size?: number }) {
  const { colors } = useTheme()
  const shown = useShownPresence()
  if (conversation.kind === 'dm') {
    const peer = conversation.member_ids.find((id) => id !== userId)
    return <Avatar id={peer ?? userId} name={title} size={size} presence={peer ? shown(peer) : undefined} surface={colors.card} />
  }
  return (
    <View className="items-center justify-center" style={{ width: size, height: size }}>
      <Icon name={conversation.kind === 'private' ? 'lock' : 'hashtag'} size={Math.round(size * 0.9)} color={colors.mutedForeground} />
    </View>
  )
}

/**
 * A message as a list shows it: avatar, author, time and the text (clamped), for Unreads, Threads, search and the
 * pinned and file lists. `flush` is the web `MessagePreview` of Unreads and Threads (28px avatar, time of day, no
 * inset); without it the row is padded and dated. `segments` marks search matches.
 */
export function MessagePreview({
  message,
  author,
  text,
  segments,
  context,
  clamp = 4,
  flush = false,
  onPress,
}: {
  message: MessageRecord
  author: string
  text: string
  segments?: readonly TextSegment[]
  /** "in #general" for a search over every conversation. */
  context?: string
  clamp?: number
  flush?: boolean
  onPress?: () => void
}) {
  const { colors } = useTheme()
  const files = message.attachments.length
  return (
    <Pressable
      accessibilityRole={onPress ? 'button' : undefined}
      disabled={!onPress}
      onPress={onPress}
      className={flush ? 'flex-row gap-2.5 active:opacity-70' : 'flex-row gap-2 p-2 rounded-md active:bg-muted/60'}
    >
      <Avatar id={message.author_id} name={author} size={flush ? 28 : 24} />
      <View className={`flex-1 min-w-0 ${flush ? '' : 'gap-0.5'}`}>
        <View className={`flex-row items-baseline ${flush ? 'gap-2' : 'gap-1.5'}`}>
          <Text numberOfLines={1} className={flush ? 'font-noto-semibold text-[13px] leading-[20px] shrink' : 'font-noto-medium text-[13px] shrink'}>
            {author}
          </Text>
          <Text className={flush ? 'text-[12px] leading-[16px] text-muted-foreground' : 'text-[11px] text-muted-foreground'}>
            {flush ? timeOfDay(message.created_at) : `${shortDate(message.created_at)} ${timeOfDay(message.created_at)}`}
          </Text>
          {context ? (
            <Text numberOfLines={1} className="text-[11px] shrink text-muted-foreground">
              {context}
            </Text>
          ) : null}
        </View>
        {message.deleted ? (
          <Text className="text-[13px] italic text-muted-foreground">This message was deleted</Text>
        ) : text || segments ? (
          // the web clamps a flush preview to 40px (`max-h-10`), a little under two 21px lines
          <Text numberOfLines={clamp} className={`text-[14px] text-foreground/85 ${flush ? `leading-[21px] ${clamp <= 2 ? 'max-h-10 overflow-hidden' : ''}` : 'leading-[20px]'}`}>
            {segments
              ? segments.map((segment, index) =>
                  segment.match ? (
                    <Text key={index} className="bg-primary/20 text-foreground leading-[21px]">
                      {segment.text}
                    </Text>
                  ) : (
                    segment.text
                  ),
                )
              : text}
          </Text>
        ) : null}
        {files > 0 ? (
          <View className="flex-row items-center gap-1">
            <Icon name="attach" size={13} color={colors.mutedForeground} />
            <Text numberOfLines={1} className="text-[12px] shrink text-muted-foreground">
              {files === 1 ? (message.attachments[0]!.deleted ? 'Deleted file' : message.attachments[0]!.file_name) : `${files} files`}
            </Text>
          </View>
        ) : null}
      </View>
    </Pressable>
  )
}
