import type { MailLabel } from '@orbit/api/generated/types.gen'
import { memo } from 'react'
import { Pressable, View } from 'react-native'

import { relativeTime } from '../../data/format'
import { personName, sentRecipients, threadSenders } from '../../data/mail'
import type { MailThreadSummary } from '../../data/mail-api'
import { alpha, starColor, useTheme } from '../../theme'
import { Avatar, Icon, Text } from '../../ui'

/**
 * A row of the thread list (the web's `ThreadRow` on a touch screen): unread dot, avatar, sender, count, draft, time,
 * subject, mailbox, snippet, labels, star and the attachment mark. A tap opens the thread; a long press offers the
 * row's actions.
 */
export const ThreadRow = memo(function ThreadRow({
  thread,
  labels,
  sentView,
  draft,
  mailboxName,
  onOpen,
  onStar,
  onLongPress,
}: {
  thread: MailThreadSummary
  labels: ReadonlyMap<string, MailLabel>
  sentView: boolean
  draft: boolean
  mailboxName?: string
  onOpen: (thread: MailThreadSummary) => void
  onStar: (thread: MailThreadSummary) => void
  onLongPress: (thread: MailThreadSummary) => void
}) {
  const { colors } = useTheme()
  const muted = alpha(colors.mutedForeground, 0.7)
  const chips = thread.labels.flatMap((id) => labels.get(id) ?? [])
  const time = (sentView && thread.last_sent_at) || thread.last_message_at
  const who = sentView ? sentRecipients(thread) : threadSenders(thread)
  const avatar = sentView && thread.sent_to[0] ? { address: thread.sent_to[0], name: thread.sent_to[0] } : { address: thread.from[0]?.address ?? '', name: personName(thread.from[0]) }
  const subject = thread.subject || '(no subject)'
  const weight = thread.unread ? 'font-noto-semibold' : ''
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${thread.unread ? 'Unread, ' : ''}${who}: ${subject}${draft ? ', draft' : ''}`}
      accessibilityHint="Long press for actions"
      onPress={() => onOpen(thread)}
      onLongPress={() => onLongPress(thread)}
      className="min-h-16 flex-row items-start gap-2 px-3 py-2.5 border-b border-border active:bg-muted"
    >
      <View className="w-2 h-8 justify-center">{thread.unread ? <View className="size-2 rounded-full bg-primary" /> : null}</View>
      {/* an outside sender's colour comes from the address: a forged From must not get a member's photo */}
      <Avatar id={avatar.address} name={avatar.name} size={32} url={null} />
      <View className="flex-1 min-w-0 gap-0.5">
        <View className="flex-row items-baseline gap-2">
          <Text numberOfLines={1} className={`shrink text-[13px] leading-[18px] ${weight}`}>
            {who}
            {thread.message_count > 1 ? <Text className="text-[12px] text-muted-foreground/70"> {thread.message_count}</Text> : null}
          </Text>
          {draft ? <Text className="text-[13px] leading-[18px] text-destructive">Draft</Text> : null}
          <View className="flex-1" />
          <Text className="text-[12px] leading-[16px] text-muted-foreground/70">{relativeTime(time)}</Text>
        </View>
        <View className="flex-row items-center gap-1.5 min-w-0">
          <Text numberOfLines={1} className={`shrink text-[13px] leading-[18px] ${weight}`}>
            {subject}
          </Text>
          {mailboxName ? (
            <View className="rounded-sm bg-muted px-1.5">
              <Text className="font-noto-medium text-[10px] leading-[15px] text-muted-foreground">{mailboxName}</Text>
            </View>
          ) : null}
        </View>
        <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground/70">
          {thread.snippet}
        </Text>
        {chips.length > 0 ? (
          <View className="mt-1 flex-row flex-wrap gap-1">
            {chips.map((label) => (
              <View key={label.id} className="h-5 max-w-32 flex-row items-center gap-1 rounded-full border border-border px-1.5">
                {/* the label's own colour, from data */}
                <View className="size-1.5 rounded-full" style={{ backgroundColor: label.color }} />
                <Text numberOfLines={1} className="shrink text-[11px] text-muted-foreground">
                  {label.name}
                </Text>
              </View>
            ))}
          </View>
        ) : null}
      </View>
      <View className="items-center gap-1">
        <Pressable accessibilityRole="button" accessibilityLabel={thread.starred ? 'Unstar' : 'Star'} accessibilityState={{ selected: thread.starred }} hitSlop={8} onPress={() => onStar(thread)} className="size-6 items-center justify-center rounded-md active:bg-muted">
          <Icon name={thread.starred ? 'starFilled' : 'star'} size={16} color={thread.starred ? starColor : muted} />
        </Pressable>
        {thread.has_attachments ? <Icon name="attach" size={14} color={muted} /> : null}
      </View>
    </Pressable>
  )
})
