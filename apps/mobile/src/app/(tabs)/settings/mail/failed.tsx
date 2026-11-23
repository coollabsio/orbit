import { useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { View } from 'react-native'

import { MailManagerPage } from '../../../../components/settings/mail/MailboxManagersOnly'
import { Badge, Note, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { mailSettingsKeys, REJECT_REASONS, type MailFailedReceipt, type MailRejectedDelivery } from '../../../../data/mail-mailboxes-api'
import { count, dateTime, mailProblem } from '../../../../data/mail-settings'
import { formatSize } from '../../../../data/workspace-settings-api'
import { useFailedReceipts, useRetryReceipt } from '../../../../hooks/mail-mailboxes'
import { useOrbit } from '../../../../orbit-context'
import { Button, EmptyState, Text } from '../../../../ui'

/**
 * The web's Settings → Mail → Failed messages: messages Orbit accepted but could not file into their mailboxes
 * (`failed`), or that have waited for over an hour without a job (`stuck`), and the deliveries no mailbox took.
 */
export default function FailedMailScreen() {
  const queryClient = useQueryClient()
  const { workspaceId } = useOrbit()
  return (
    <MailManagerPage title="Failed messages" onRefresh={() => queryClient.refetchQueries({ queryKey: mailSettingsKeys.receipts(workspaceId) })}>
      <FailedMail />
    </MailManagerPage>
  )
}

function FailedMail() {
  const receipts = useFailedReceipts()
  if (receipts.isPending) {
    return (
      <SettingsCard title="Failed messages">
        <Note loading>Loading messages…</Note>
      </SettingsCard>
    )
  }
  if (receipts.isError) {
    return (
      <SettingsCard title="Failed messages">
        <View className="flex-row items-center gap-2">
          <View className="min-w-0 flex-1">
            <Note error>{mailProblem(receipts.error, 'Failed messages could not be loaded.')}</Note>
          </View>
          <Button label="Retry" variant="ghost" onPress={() => void receipts.refetch()} />
        </View>
      </SettingsCard>
    )
  }
  const { count: total, receipts: rows, rejected } = receipts.data
  if (total === 0 && rejected.length === 0) return <EmptyState icon="smsTracking" title="No failed messages" description="Every message Orbit accepted reached its mailboxes." />
  const shown = rows.length < total ? ` The newest ${rows.length} are shown.` : ''
  return (
    <>
      {total > 0 ? (
        <SettingsCard title="Failed messages" description={`${count(total, 'message', 'messages')} did not reach their mailboxes. Orbit keeps each one; fix the cause, then retry.${shown}`} flush>
          {rows.map((receipt, index) => (
            <ReceiptRow key={receipt.id} receipt={receipt} first={index === 0} />
          ))}
        </SettingsCard>
      ) : null}
      {rejected.length > 0 ? <NotDelivered rejected={rejected} /> : null}
    </>
  )
}

/** The web's table row as a list row: a phone has no room for its six columns. */
function ReceiptRow({ receipt, first }: { receipt: MailFailedReceipt; first: boolean }) {
  const retry = useRetryReceipt()
  const [notice, setNotice] = useState<{ text: string; error?: boolean }>()
  const sender = receipt.mail_from ?? 'Null sender <>'
  return (
    <SettingsRow first={first}>
      <View className="min-w-0 flex-1 gap-1">
        <View className="flex-row flex-wrap items-center gap-2">
          <Badge label={receipt.status === 'failed' ? 'Failed' : 'Stuck'} tone={receipt.status === 'failed' ? 'destructive' : 'outline'} />
          <Text numberOfLines={1} className="min-w-0 shrink font-noto-medium text-[13px]">
            {sender}
          </Text>
        </View>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">
          {`${dateTime(receipt.received_at)} · ${formatSize(receipt.size)} · ${count(receipt.attempts, 'attempt', 'attempts')}`}
        </Text>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">
          {receipt.error ?? (receipt.status === 'stuck' ? 'Waiting for over an hour without a job.' : 'No error was recorded.')}
        </Text>
        {notice ? <Note error={notice.error}>{notice.text}</Note> : null}
      </View>
      <Button
        label={retry.isPending ? 'Retrying…' : 'Retry'}
        variant="outline"
        size="sm"
        disabled={retry.isPending}
        onPress={() => {
          setNotice(undefined)
          retry.mutate(receipt.id, {
            onSuccess: () => setNotice({ text: 'Orbit files the message again' }),
            onError: (error) => setNotice({ text: mailProblem(error, 'The message could not be retried.'), error: true }),
          })
        }}
      />
    </SettingsRow>
  )
}

/** Mail a provider accepted that Orbit could not deliver, so the sender got no bounce. Kept for 30 days. */
function NotDelivered({ rejected }: { rejected: MailRejectedDelivery[] }) {
  return (
    <SettingsCard
      title="Not delivered"
      description="A provider accepted these messages, but no mailbox could take them, so the sender was not told. Orbit kept no copy. They stay listed for 30 days."
      flush
    >
      {rejected.map((entry, index) => (
        <SettingsRow key={`${entry.received_at}:${entry.recipient}`} first={index === 0}>
          <View className="min-w-0 flex-1 gap-0.5">
            <Text numberOfLines={1} className="font-noto-medium text-[13px]">
              {entry.recipient}
            </Text>
            <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">
              {`From ${entry.mail_from ?? 'Null sender <>'} · ${dateTime(entry.received_at)}`}
            </Text>
            <Text className="text-[12px] leading-[16px] text-muted-foreground">{`${REJECT_REASONS[entry.reason]} (${entry.provider_name})`}</Text>
          </View>
        </SettingsRow>
      ))}
    </SettingsCard>
  )
}
