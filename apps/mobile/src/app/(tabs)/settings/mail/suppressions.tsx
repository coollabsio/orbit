import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import { View } from 'react-native'

import { MailManagerPage } from '../../../../components/settings/mail/MailboxManagersOnly'
import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { Badge, Note, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { mailSettingsKeys, SUPPRESSION_REASONS, type MailSuppression } from '../../../../data/mail-mailboxes-api'
import { mailProblem, since } from '../../../../data/mail-settings'
import { useAddSuppression, useRemoveSuppression, useSuppressions } from '../../../../hooks/mail-mailboxes'
import { useOrbit } from '../../../../orbit-context'
import { Button, EmptyState, Input, Text } from '../../../../ui'

/** The web's Settings → Mail → Suppressions: addresses Orbit no longer sends to, with search, **Add** and **Remove**. */
export default function MailSuppressionsScreen() {
  const queryClient = useQueryClient()
  const { workspaceId } = useOrbit()
  return (
    <MailManagerPage title="Suppressions" onRefresh={() => queryClient.refetchQueries({ queryKey: mailSettingsKeys.suppressions(workspaceId) })}>
      <Suppressions />
    </MailManagerPage>
  )
}

function Suppressions() {
  const [query, setQuery] = useState('')
  const [search, setSearch] = useState('')
  useEffect(() => {
    const timer = setTimeout(() => setSearch(query), 250)
    return () => clearTimeout(timer)
  }, [query])
  const list = useSuppressions(search)
  const remove = useRemoveSuppression()
  const add = useAddSuppression()
  const [address, setAddress] = useState('')
  const [note, setNote] = useState('')
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  // the web's toasts
  const [notice, setNotice] = useState<{ text: string; error?: boolean }>()
  const rows = list.data ?? []

  const removeRow = (row: MailSuppression) =>
    setConfirm({
      title: `Send to ${row.address} again?`,
      description: 'Orbit sends to it again. If it bounces or complains again, it comes back on the list.',
      confirmLabel: 'Remove',
      onConfirm: () => {
        setNotice(undefined)
        remove.mutate(row.address, {
          onSuccess: () => setNotice({ text: `${row.address} removed` }),
          onError: (error) => setNotice({ text: mailProblem(error, 'The address could not be removed.'), error: true }),
        })
      },
    })

  const addAddress = () => {
    const trimmed = address.trim()
    if (!trimmed || add.isPending) return
    setNotice(undefined)
    add.mutate({ address: trimmed, detail: note.trim() || undefined }, {
      onSuccess: (entry) => {
        setAddress('')
        setNote('')
        setNotice({ text: `${entry.address} added` })
      },
      onError: (error) => setNotice({ text: mailProblem(error, 'The address could not be added.'), error: true }),
    })
  }

  return (
    <SettingsCard
      title="Suppressions"
      description="Addresses that bounced hard, marked your mail as spam or that you added. Orbit does not send to them, so your domains keep a good reputation. Remove an address only when you know it works again."
      flush
    >
      <SettingsRow first>
        <View className="min-w-0 flex-1 gap-2">
          <Input
            accessibilityLabel="Address to suppress"
            placeholder="name@example.com"
            value={address}
            onChangeText={setAddress}
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="email-address"
            returnKeyType="next"
          />
          <Input
            accessibilityLabel="Note"
            placeholder="Note (optional)"
            value={note}
            onChangeText={setNote}
            maxLength={500}
            returnKeyType="done"
            onSubmitEditing={addAddress}
          />
          <Button label="Add address" disabled={!address.trim() || add.isPending} onPress={addAddress} />
        </View>
      </SettingsRow>
      <SettingsRow>
        <View className="min-w-0 flex-1">
          <Input
            accessibilityLabel="Search suppressed addresses"
            placeholder="Search addresses"
            value={query}
            onChangeText={setQuery}
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="email-address"
            returnKeyType="search"
          />
        </View>
      </SettingsRow>
      {notice ? (
        <SettingsRow>
          <Note error={notice.error}>{notice.text}</Note>
        </SettingsRow>
      ) : null}
      {list.isPending ? (
        <SettingsRow>
          <Note loading>Loading addresses…</Note>
        </SettingsRow>
      ) : null}
      {list.isError ? (
        <SettingsRow>
          <View className="min-w-0 flex-1">
            <Note error>{mailProblem(list.error, 'The addresses could not be loaded.')}</Note>
          </View>
          <Button label="Retry" variant="ghost" onPress={() => void list.refetch()} />
        </SettingsRow>
      ) : null}
      {list.isSuccess && rows.length === 0 ? (
        <View className="border-t border-border">
          <EmptyState
            icon="shieldCross"
            title={search.trim() ? 'No address matches' : 'No suppressed addresses'}
            description={search.trim() ? undefined : 'An address shows here when it bounces hard, when its owner marks your mail as spam, or when you add it.'}
          />
        </View>
      ) : null}
      {rows.map((row) => (
        <SettingsRow key={row.address}>
          <View className="min-w-0 flex-1 gap-0.5">
            <View className="flex-row flex-wrap items-center gap-2">
              <Text selectable className="min-w-0 shrink font-mono text-[12px] leading-[16px]">
                {row.address}
              </Text>
              <Badge label={SUPPRESSION_REASONS[row.reason] ?? row.reason} tone={row.reason === 'complaint' ? 'destructive' : 'outline'} />
            </View>
            <Text className="text-[12px] leading-[16px] text-muted-foreground">Added {since(row.created_at)}</Text>
            {row.detail ? <Text className="text-[12px] leading-[16px] text-muted-foreground">{row.detail}</Text> : null}
          </View>
          <Button label="Remove" variant="ghost" size="sm" disabled={remove.isPending} onPress={() => removeRow(row)} />
        </SettingsRow>
      ))}
      {list.hasNextPage ? (
        <View className="items-center border-t border-border p-2">
          <Button label={list.isFetchingNextPage ? 'Loading…' : 'Show more'} variant="ghost" disabled={list.isFetchingNextPage} onPress={() => void list.fetchNextPage()} />
        </View>
      ) : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}
