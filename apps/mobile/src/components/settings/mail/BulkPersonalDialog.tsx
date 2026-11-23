import { useState } from 'react'
import { View } from 'react-native'

import { NAME_PATTERNS, proposalRows, type MailDomain, type MailNamePattern } from '../../../data/mail-mailboxes-api'
import { count, mailProblem } from '../../../data/mail-settings'
import { useBulkPersonalPreview, useMailboxWrites } from '../../../hooks/mail-mailboxes'
import { Button, Input, Text } from '../../../ui'
import { Dialog } from '../Dialog'
import { Badge, FieldLabel, Note, Select } from '../Shell'

/**
 * **Create a mailbox for each member**: the admin picks a domain and a pattern, Orbit proposes an address for each
 * member without a personal mailbox and marks the conflicts, the admin edits any address, then confirms. Members whose
 * address still conflicts get no mailbox.
 */
export function BulkPersonalDialog({ domains, onClose, onCreated }: { domains: readonly MailDomain[]; onClose: () => void; onCreated: (text: string) => void }) {
  const usable = [...domains].sort((a, b) => Number(b.status === 'verified') - Number(a.status === 'verified'))
  const [domainId, setDomainId] = useState(usable[0]?.id ?? '')
  const [pattern, setPattern] = useState<MailNamePattern>('first_dot_last')
  /** The admin's local parts, by membership; cleared when the domain or the pattern changes. */
  const [edits, setEdits] = useState<Record<string, string>>({})
  /** The local parts the server refused at the last confirm, by membership. */
  const [refused, setRefused] = useState<Record<string, string>>({})
  const [created, setCreated] = useState<string>()
  const preview = useBulkPersonalPreview(domainId, pattern)
  const { bulk } = useMailboxWrites()
  const domain = domains.find((entry) => entry.id === domainId)
  const marked = proposalRows(preview.data ?? [], edits, refused)
  const ready = marked.filter((row) => row.problem === null).length
  const conflicts = marked.length - ready

  const reset = () => {
    setEdits({})
    setRefused({})
  }

  const confirm = () => {
    if (!domainId || ready === 0 || bulk.isPending) return
    const overrides = marked.filter((row) => row.edited).map((row) => ({ membership_id: row.membership_id, local_part: row.localPart }))
    bulk.mutate(
      { domain_id: domainId, pattern, overrides },
      {
        onSuccess: (result) => {
          const text = `${count(result.created.length, 'mailbox', 'mailboxes')} created`
          if (result.conflicts.length === 0) {
            onCreated(text)
            onClose()
            return
          }
          setCreated(text)
          const asked = new Map(marked.map((row) => [row.membership_id, row.localPart]))
          setRefused(Object.fromEntries(result.conflicts.map((conflict) => [conflict.membership_id, asked.get(conflict.membership_id) ?? conflict.local_part])))
        },
      },
    )
  }

  return (
    <Dialog
      visible
      title="Create a mailbox for each member"
      description="Each member without a personal mailbox gets one, named after them. Edit any address before you confirm; members marked with a conflict are skipped."
      onClose={bulk.isPending ? () => {} : onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} disabled={bulk.isPending} />
          <Button label={bulk.isPending ? 'Creating…' : `Create ${count(ready, 'mailbox', 'mailboxes')}`} disabled={ready === 0 || bulk.isPending} onPress={confirm} />
        </>
      }
    >
      <View className="gap-4">
        <View className="gap-1.5">
          <FieldLabel>Domain</FieldLabel>
          <Select
            label="Domain"
            value={domainId}
            options={usable.map((entry) => ({ value: entry.id, label: entry.status === 'verified' ? entry.name : `${entry.name} (${entry.status})` }))}
            onChange={(value) => {
              setDomainId(value)
              reset()
            }}
          />
        </View>
        <View className="gap-1.5">
          <FieldLabel>Pattern</FieldLabel>
          <Select
            label="Pattern"
            value={pattern}
            options={NAME_PATTERNS}
            onChange={(value) => {
              setPattern(value)
              reset()
            }}
          />
        </View>

        <View accessibilityLabel="Proposed addresses" className="rounded-lg border border-border">
          {preview.isPending && domainId ? (
            <View className="p-4">
              <Note loading>Making addresses…</Note>
            </View>
          ) : null}
          {preview.isError ? (
            <View className="p-4">
              <Note error>{mailProblem(preview.error, 'The addresses could not be made.')}</Note>
            </View>
          ) : null}
          {preview.isSuccess && marked.length === 0 ? (
            <View className="p-4">
              <Note>Every member has a personal mailbox.</Note>
            </View>
          ) : null}
          {marked.map((row, index) => (
            <View key={row.membership_id} className={`gap-1.5 px-3 py-2.5 ${index === 0 ? '' : 'border-t border-border'}`}>
              <View className="flex-row items-center gap-2">
                <Text numberOfLines={1} className="min-w-0 shrink font-noto-medium text-[13px]">
                  {row.name}
                </Text>
                {row.problem ? <Badge label={row.problem} tone="destructive" /> : null}
              </View>
              <View className="flex-row items-center gap-1.5">
                <View className="min-w-0 flex-1">
                  <Input
                    accessibilityLabel={`Address of ${row.name}`}
                    autoComplete="off"
                    autoCapitalize="none"
                    autoCorrect={false}
                    keyboardType="email-address"
                    value={row.localPart}
                    onChangeText={(value) => setEdits((current) => ({ ...current, [row.membership_id]: value.trim().toLowerCase() }))}
                  />
                </View>
                <Text numberOfLines={1} className="max-w-[50%] text-muted-foreground">
                  @{domain?.name}
                </Text>
              </View>
            </View>
          ))}
        </View>

        {created ? <Note>{created}</Note> : null}
        {bulk.isError ? <Note error>{mailProblem(bulk.error, 'The mailboxes were not created.')}</Note> : null}
        {marked.length > 0 ? (
          <Text className="text-[12px] text-muted-foreground">
            {`${count(ready, 'mailbox', 'mailboxes')} to create${conflicts > 0 ? `, ${count(conflicts, 'conflict', 'conflicts')} skipped` : ''}`}
          </Text>
        ) : null}
      </View>
    </Dialog>
  )
}
