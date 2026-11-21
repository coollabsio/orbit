import type { MailMessageView } from '@orbit/api/generated/types.gen'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { ActivityIndicator, Alert, Pressable, View } from 'react-native'

import { fullDate, timeOfDay } from '../../data/format'
import { attachmentPath, domainOf, firstLine, peopleLine, personName, splitTextQuote } from '../../data/mail'
import * as mail from '../../data/mail-api'
import { deliverySummary, RECIPIENT_LABEL, recipientProblem, recipientState, type RecipientState } from '../../data/mail-status'
import { fileSize } from '../../data/tasks'
import { useOrbit } from '../../orbit-context'
import { useMembers } from '../../queries'
import { errorMessage } from '../../session-context'
import { type Colors, useTheme } from '../../theme'
import { Avatar, Button, Icon, type IconName, LabelTip, Text } from '../../ui'
import { openFile } from '../tasks/files'
import { MailHtmlView } from './MailHtmlView'

/** One message of a thread: a header that opens and folds it, then the delivery, the warnings, the body and the files. */
export function MessageItem({ message, expanded, onToggle }: { message: MailMessageView; expanded: boolean; onToggle: () => void }) {
  const { colors } = useTheme()
  const { client, workspaceId } = useOrbit()
  const members = useMembers().data
  const [imagesLoaded, setImagesLoaded] = useState(false)
  const [blocked, setBlocked] = useState<number | null>(null)
  const senderPref = useMutation({ mutationFn: (address: string) => mail.setSenderImages(client, workspaceId, address) })
  const loadImages = imagesLoaded || message.load_images_default
  const address = message.from?.address
  const name = personName(message.from)
  const date = message.sent_at ?? message.received_at
  // Orbit sent it, or DMARC confirmed the domain: only then may the sender get a member's photo
  const trusted = Boolean(message.delivery) || Boolean(message.auth.aligned_domain)
  const member = trusted && address ? members?.find((entry) => entry.email.toLowerCase() === address) : undefined
  const files = message.attachments.filter((attachment) => !attachment.inline)
  const offerImages = message.has_html && !loadImages && (blocked ?? 0) > 0

  return (
    <View className={`border-b border-border pt-2.5 ${expanded ? 'pb-4' : 'pb-2.5'}`}>
      <Pressable accessibilityRole="button" accessibilityState={{ expanded }} onPress={onToggle} className="flex-row items-start gap-2.5 rounded-md active:bg-muted">
        {member ? <Avatar id={member.user_id} name={member.display_name} size={32} /> : <Avatar id={address ?? name} name={name} size={32} url={null} />}
        <View className="flex-1 min-w-0">
          <View className="flex-row items-baseline gap-2 min-w-0">
            <Text numberOfLines={1} className="flex-1 font-noto-semibold text-[13px] leading-[18px]">
              {name}
            </Text>
            <Text className="text-[12px] leading-[18px] text-muted-foreground/70">
              {fullDate(date)}, {timeOfDay(date)}
            </Text>
          </View>
          <View className="flex-row items-center gap-1.5 min-w-0">
            <Text numberOfLines={1} className="shrink text-[12px] leading-[18px] text-muted-foreground/70">
              {address}
            </Text>
            {message.auth.aligned_domain ? (
              <View accessibilityLabel={`DMARC confirmed that the message comes from ${message.auth.aligned_domain}`} className="flex-row items-center gap-0.5">
                <Icon name="shield" size={13} color={colors.mutedForeground} />
                <Text className="text-[12px] leading-[18px] text-muted-foreground">{message.auth.aligned_domain}</Text>
              </View>
            ) : null}
          </View>
          {expanded ? null : (
            <Text numberOfLines={1} className="text-[13px] leading-[18px] text-muted-foreground">
              {firstLine(message.body_text)}
            </Text>
          )}
        </View>
      </Pressable>
      {expanded ? (
        <View>
          <View className="mt-2 gap-0.5 pl-[42px]">
            {message.to.length > 0 ? <Text numberOfLines={2} className="text-[12px] leading-[16px] text-muted-foreground">To: {peopleLine(message.to)}</Text> : null}
            {message.cc.length > 0 ? <Text numberOfLines={2} className="text-[12px] leading-[16px] text-muted-foreground">Cc: {peopleLine(message.cc)}</Text> : null}
            {message.sender ? <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">Sent by {message.sender}</Text> : null}
          </View>
          {message.delivery ? <DeliveryStatus outboundId={message.delivery.outbound_id} recipients={message.delivery.recipients} /> : null}
          <SafetyBanners message={message} />
          {offerImages ? (
            <View className="mt-3 flex-row flex-wrap items-center gap-2 rounded-lg border border-border px-2.5 py-1.5">
              <Icon name="gallery" size={16} color={colors.mutedForeground} />
              <Text className="flex-1 text-[13px] text-muted-foreground">Remote images are hidden.</Text>
              <Button label="Load images" variant="outline" size="sm" onPress={() => setImagesLoaded(true)} />
              {message.auth.dmarc === 'pass' && address ? (
                <Button
                  label="Always for this sender"
                  variant="ghost"
                  size="sm"
                  disabled={senderPref.isPending}
                  onPress={() =>
                    senderPref.mutate(address, {
                      onSuccess: () => setImagesLoaded(true),
                      onError: (error) => Alert.alert('The setting could not be saved.', errorMessage(error)),
                    })
                  }
                />
              ) : null}
            </View>
          ) : null}
          <View className="mt-3">{message.has_html ? <MailHtmlView messageId={message.id} loadImages={loadImages} onBlockedImages={setBlocked} /> : <PlainBody text={message.body_text} />}</View>
          {files.length > 0 ? <MailFiles files={files} /> : null}
        </View>
      ) : null}
    </View>
  )
}

/** A plain-text body; its trailing quote folds behind "•••", as in the HTML view. */
function PlainBody({ text }: { text: string }) {
  const [open, setOpen] = useState(false)
  const { body, quote } = splitTextQuote(text)
  return (
    <View className="items-start gap-2">
      <Text selectable className="text-[14px] leading-[22px]">
        {body}
      </Text>
      {quote ? (
        <>
          <Pressable accessibilityRole="button" accessibilityLabel={open ? 'Hide quoted text' : 'Show quoted text'} accessibilityState={{ expanded: open }} onPress={() => setOpen(!open)} className="h-[22px] justify-center rounded-full border border-border bg-muted/50 px-2.5 active:bg-muted">
            <Text className="text-[12px] tracking-widest text-muted-foreground">•••</Text>
          </Pressable>
          {open ? (
            <View className="border-l-2 border-border pl-3">
              <Text selectable className="text-[14px] leading-[22px] text-muted-foreground">
                {quote}
              </Text>
            </View>
          ) : null}
        </>
      ) : null}
    </View>
  )
}

function Banner({ icon, title, danger = false, children }: { icon: IconName; title: string; danger?: boolean; children: string }) {
  const { colors } = useTheme()
  const tone = danger ? colors.destructive : colors.foreground
  return (
    <View accessibilityRole="alert" className={`flex-row gap-2.5 rounded-lg border px-3 py-2.5 ${danger ? 'border-destructive/30 bg-destructive/5' : 'border-border bg-card'}`}>
      <Icon name={icon} size={16} color={tone} />
      <View className="flex-1 gap-0.5">
        <Text className={`font-noto-medium text-[13px] leading-[18px] ${danger ? 'text-destructive' : ''}`}>{title}</Text>
        <Text className={`text-[13px] leading-[18px] ${danger ? 'text-destructive/90' : 'text-muted-foreground'}`}>{children}</Text>
      </View>
    </View>
  )
}

/**
 * The warnings over a message (the web's `SafetyBanner`): failed DMARC, a sender with a member's name, an outside reply
 * in an inside thread, an external sender, and a message shown as plain text only.
 */
function SafetyBanners({ message }: { message: MailMessageView }) {
  const address = message.from?.address
  const domain = domainOf(address)
  const banners: { key: string; icon: IconName; title: string; danger?: boolean; text: string }[] = []
  if (message.auth.dmarc === 'fail') {
    banners.push({ key: 'auth', icon: 'shieldCross', title: 'Failed authentication', danger: true, text: `This message says it is from ${domain || 'a domain'}, but that domain did not confirm it (DMARC). It may be forged: do not trust its links or requests.` })
  }
  if (message.lookalike_member) {
    banners.push({ key: 'lookalike', icon: 'profileCircle', title: 'Lookalike name', danger: true, text: `The sender uses the name of ${message.lookalike_member}, but ${address ?? 'this address'} is not theirs.` })
  }
  if (message.external) {
    banners.push({ key: 'joined', icon: 'danger', title: 'Outside reply', text: 'This message came from outside the workspace into a conversation that started inside it. Check the sender before you act on it.' })
  } else if (message.from_external) {
    banners.push({ key: 'external', icon: 'global', title: 'External sender', text: `${domain || 'The sender'} is outside this workspace. Be careful with links and attachments.` })
  }
  if (message.warning) {
    banners.push({
      key: 'warning',
      icon: 'docText',
      title: 'Shown as plain text',
      text: `${message.warning === 'limit' ? 'This message is too large or too complex to show in full.' : 'This message could not be read in full.'} Open it in Orbit on the web to download the original.`,
    })
  }
  if (banners.length === 0) return null
  return (
    <View className="mt-3 gap-2">
      {banners.map((banner) => (
        <Banner key={banner.key} icon={banner.icon} title={banner.title} danger={banner.danger}>
          {banner.text}
        </Banner>
      ))}
    </View>
  )
}

/** The theme has no green: delivered is the text colour, a failure the destructive one. */
const toneOf = (state: RecipientState, colors: Colors) =>
  state === 'delivered' ? colors.foreground : state === 'bounced' || state === 'suppressed' ? colors.destructive : colors.mutedForeground
const ICON: Record<RecipientState, IconName> = { sending: 'clock', delivered: 'checkCircle', delayed: 'warning', bounced: 'closeCircle', suppressed: 'closeCircle', cancelled: 'clock' }

const DELIVERY_HELP =
  'Delivered means the next mail server accepted the message. Orbit does not track whether it was opened. Delayed: the receiving server has not accepted it for more than 4 hours; Orbit keeps trying for 5 days.'

/** The delivery of a sent message (web §9.1 U10): the summary, then each recipient; a bounced one says why and offers Retry. */
function DeliveryStatus({ outboundId, recipients }: { outboundId: string; recipients: NonNullable<MailMessageView['delivery']>['recipients'] }) {
  const { colors } = useTheme()
  const { client, workspaceId } = useOrbit()
  const summary = deliverySummary(recipients)
  const shown = recipients.filter((recipient) => recipient.status !== 'cancelled')
  const [chosen, setChosen] = useState<boolean | null>(null)
  const open = chosen ?? (summary.label !== 'Sending' && summary.label !== 'Delivered')
  const retry = useMutation({ mutationFn: (recipientId: string) => mail.retryRecipient(client, workspaceId, outboundId, recipientId) })
  if (shown.length === 0) return null
  return (
    <View className="mt-2 gap-1 pl-[42px]">
      <LabelTip label="Delivery" text={DELIVERY_HELP} className="text-[12px] leading-[16px] text-muted-foreground" />
      <View className="flex-row items-center gap-1.5">
        <Pressable accessibilityRole="button" accessibilityState={{ expanded: open }} onPress={() => setChosen(!open)} className="flex-row items-center gap-1.5 rounded-md py-0.5 active:bg-muted">
          <Icon name={ICON[summary.state]} size={14} color={toneOf(summary.state, colors)} />
          <Text className="font-noto-medium text-[12px]" style={{ color: toneOf(summary.state, colors) }}>
            {summary.label}
          </Text>
          {summary.failed > 0 && shown.length > 1 ? <Text className="text-[12px] text-muted-foreground">· {summary.failed} not delivered</Text> : null}
          <Icon name={open ? 'chevronUp' : 'chevronDown'} size={12} color={colors.mutedForeground} />
        </Pressable>
      </View>
      {open
        ? shown.map((recipient) => {
            const state = recipientState(recipient)
            const problem = recipientProblem(recipient)
            return (
              <View key={recipient.id} className="gap-0.5">
                <View className="flex-row flex-wrap items-center gap-x-2">
                  <Icon name={ICON[state]} size={14} color={toneOf(state, colors)} />
                  <Text numberOfLines={1} className="shrink text-[12px]">
                    {recipient.address}
                  </Text>
                  {recipient.kind === 'bcc' ? <Text className="rounded border border-border px-1 text-[10px] text-muted-foreground">Bcc</Text> : null}
                  <Text className="text-[12px]" style={{ color: toneOf(state, colors) }}>
                    {RECIPIENT_LABEL[state]}
                  </Text>
                  {problem && recipient.retryable ? (
                    <Button
                      label="Retry"
                      icon="refresh"
                      size="xs"
                      variant="outline"
                      disabled={retry.isPending && retry.variables === recipient.id}
                      onPress={() =>
                        retry.mutate(recipient.id, {
                          onSuccess: () => Alert.alert(`Sending to ${recipient.address} again.`),
                          onError: (error) => Alert.alert('The message could not be sent again.', errorMessage(error)),
                        })
                      }
                    />
                  ) : null}
                </View>
                {problem ? <Text className="pl-5 text-[12px] text-muted-foreground">{problem}</Text> : null}
              </View>
            )
          })
        : null}
    </View>
  )
}

/** The files of a message: a tap downloads one with the session (never in the URL) and opens the system preview. */
function MailFiles({ files }: { files: MailMessageView['attachments'] }) {
  const { colors } = useTheme()
  const { session, workspaceId } = useOrbit()
  const [opening, setOpening] = useState<string | null>(null)
  const open = async (file: MailMessageView['attachments'][number]) => {
    if (opening) return
    const name = file.filename?.trim() || 'attachment'
    setOpening(file.id)
    try {
      await openFile(`${session.server}${attachmentPath(workspaceId, file.id)}`, name, session.token)
    } catch (error) {
      Alert.alert(`Could not open “${name}”`, errorMessage(error))
    } finally {
      setOpening(null)
    }
  }
  return (
    <View className="mt-3 gap-1.5">
      {files.map((file) => (
        <Pressable key={file.id} accessibilityRole="button" accessibilityLabel={`Open ${file.filename || 'attachment'}`} onPress={() => void open(file)} className="self-start max-w-full min-h-9 flex-row items-center gap-2 px-2.5 rounded-md border border-border bg-card active:bg-muted">
          {opening === file.id ? <ActivityIndicator size="small" color={colors.mutedForeground} /> : <Icon name="documentDownload" size={16} color={colors.mutedForeground} />}
          <Text numberOfLines={1} className="shrink font-noto-medium text-[13px]">
            {file.filename?.trim() || 'attachment'}
          </Text>
          <Text className="text-[12px] text-muted-foreground">{fileSize(file.size)}</Text>
        </Pressable>
      ))}
    </View>
  )
}
