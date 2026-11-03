import { router } from 'expo-router'
import type { ReactNode } from 'react'
import { Pressable, View } from 'react-native'

import type { TextSegment } from '../../data/chat'
import { shortDate, timeOfDay } from '../../data/format'
import type { ConversationRecord, MessageRecord } from '../../data/orbit'
import { useTheme } from '../../theme'
import { Avatar, Icon, IconButton, SafeAreaView, Text } from '../../ui'

/** A chat screen with the web's 48px pane header: Back, the title, and actions at the end. */
export function ChatPane({ title, subtitle, actions, children }: { title: string; subtitle?: string; actions?: ReactNode; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-card">
      <View className="h-12 flex-row items-center gap-1 px-2">
        <IconButton icon="back" label="Back" size={32} iconSize={20} color={colors.mutedForeground} onPress={() => (router.canGoBack() ? router.back() : router.navigate('/chat'))} />
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

export function ConversationGlyph({ conversation, title, userId, size = 16 }: { conversation: ConversationRecord; title: string; userId: string; size?: number }) {
  const { colors } = useTheme()
  if (conversation.kind === 'dm') return <Avatar id={conversation.member_ids.find((id) => id !== userId) ?? userId} name={title} size={size + 2} />
  return <Icon name={conversation.kind === 'private' ? 'lock' : 'hashtag'} size={size} color={colors.mutedForeground} />
}

/**
 * The web `MessagePreview`: avatar, author, time and the text (clamped), for Unreads, Threads, search and the pinned
 * and file lists. `segments` marks search matches.
 */
export function MessagePreview({
  message,
  author,
  text,
  segments,
  context,
  clamp = 4,
  onPress,
}: {
  message: MessageRecord
  author: string
  text: string
  segments?: readonly TextSegment[]
  /** "in #general" for a search over every conversation. */
  context?: string
  clamp?: number
  onPress?: () => void
}) {
  const { colors } = useTheme()
  const files = message.attachments.length
  return (
    <Pressable
      accessibilityRole={onPress ? 'button' : undefined}
      disabled={!onPress}
      onPress={onPress}
      className="flex-row gap-2 p-2 rounded-md active:bg-muted/60"
    >
      <Avatar id={message.author_id} name={author} size={24} />
      <View className="flex-1 min-w-0 gap-0.5">
        <View className="flex-row items-baseline gap-1.5">
          <Text numberOfLines={1} className="font-noto-medium text-[13px] shrink">
            {author}
          </Text>
          <Text className="text-[11px] text-muted-foreground">
            {shortDate(message.created_at)} {timeOfDay(message.created_at)}
          </Text>
          {context ? (
            <Text numberOfLines={1} className="text-[11px] shrink text-muted-foreground">
              {context}
            </Text>
          ) : null}
        </View>
        {text || segments ? (
          <Text numberOfLines={clamp} className="text-[14px] leading-[20px] text-foreground/85">
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
