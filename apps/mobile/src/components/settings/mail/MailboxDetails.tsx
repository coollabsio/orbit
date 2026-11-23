import { useState } from 'react'
import { View } from 'react-native'

import type { MailDomain, MailMailbox } from '../../../data/mail-mailboxes-api'
import { mailProblem } from '../../../data/mail-settings'
import type { MemberRecord } from '../../../data/orbit'
import { useMailboxWrites } from '../../../hooks/mail-mailboxes'
import { Button, Input, Text } from '../../../ui'
import { MemberPicker } from '../../chat/ChannelSheets'
import { ConfirmDialog, type ConfirmRequest } from '../Dialog'
import { Badge, Note } from '../Shell'
import { AddressFields, pickerPeople } from './MailboxForm'

type Notice = { text: string; error?: boolean }

/** One mailbox opened from the list: name, addresses, members, suspend and delete. */
export function MailboxDetails({ mailbox, domains, members }: { mailbox: MailMailbox; domains: readonly MailDomain[]; members: readonly MemberRecord[] }) {
  const { rename, addAlias, removeAddress, addMember, removeMember, setStatus, remove } = useMailboxWrites()
  const [name, setName] = useState(mailbox.name)
  const [aliasLocal, setAliasLocal] = useState('')
  const [aliasDomain, setAliasDomain] = useState(domains[0]?.id ?? '')
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  // the web's toasts
  const [notice, setNotice] = useState<Notice>()
  const ownerLeft = mailbox.kind === 'personal' && !mailbox.owner_membership_id
  const owner = members.find((member) => member.id === mailbox.owner_membership_id)
  const readerIds = mailbox.member_ids.flatMap((membershipId) => members.find((member) => member.id === membershipId)?.user_id ?? [])
  const trimmed = name.trim()

  const said = (text: string) => () => setNotice({ text })
  const failed = (fallback: string) => (error: unknown) => setNotice({ text: mailProblem(error, fallback), error: true })
  const run = (start: () => void) => {
    setNotice(undefined)
    start()
  }

  const saveName = () => {
    if (!trimmed || trimmed === mailbox.name) return
    run(() => rename.mutate({ mailboxId: mailbox.id, name: trimmed }, { onSuccess: said('Mailbox renamed'), onError: failed('The mailbox could not be renamed.') }))
  }

  const submitAlias = () => {
    if (!aliasLocal || !aliasDomain) return
    run(() => addAlias.mutate({ mailboxId: mailbox.id, domainId: aliasDomain, localPart: aliasLocal }, { onSuccess: () => setAliasLocal(''), onError: failed('The alias could not be added.') }))
  }

  /** `MemberPicker` reports the whole new list; one person changes at a time. */
  const changeReaders = (next: string[]) => {
    const membershipOf = (userId: string | undefined) => members.find((member) => member.user_id === userId)?.id
    const added = membershipOf(next.find((userId) => !readerIds.includes(userId)))
    const removed = membershipOf(readerIds.find((userId) => !next.includes(userId)))
    setNotice(undefined)
    if (added) addMember.mutate({ mailboxId: mailbox.id, membershipId: added }, { onError: failed('The member could not be added.') })
    if (removed) removeMember.mutate({ mailboxId: mailbox.id, membershipId: removed }, { onError: failed('The member could not be removed.') })
  }

  const deleteMailbox = () =>
    setConfirm({
      title: `Delete ${mailbox.name}?`,
      description: 'Its addresses stop receiving mail and its threads are deleted for every member. You cannot undo this.',
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => run(() => remove.mutate(mailbox.id, { onError: failed('The mailbox could not be deleted.') })),
    })

  return (
    <View className="gap-5 border-t border-border bg-muted/30 px-4 py-4">
      <View className="flex-row items-end gap-2">
        <View className="min-w-0 flex-1">
          <Input label="Name" maxLength={80} value={name} onChangeText={setName} returnKeyType="done" onSubmitEditing={saveName} />
        </View>
        <Button label="Rename" variant="outline" disabled={!trimmed || trimmed === mailbox.name || rename.isPending} onPress={saveName} />
      </View>

      <View accessibilityLabel="Addresses" className="gap-2">
        <Text className="font-noto-medium text-[12px] text-muted-foreground">Addresses</Text>
        {mailbox.addresses.map((address) => (
          <View key={address.id} className="min-h-7 flex-row items-center gap-2">
            <Text selectable numberOfLines={1} className="min-w-0 shrink font-mono text-[12px]">
              {address.address}
            </Text>
            <Badge label={address.kind === 'primary' ? 'Primary' : 'Alias'} tone={address.kind === 'primary' ? 'secondary' : 'outline'} />
            {address.kind === 'alias' ? (
              <Button
                label="Remove"
                variant="ghost"
                size="sm"
                disabled={removeAddress.isPending}
                onPress={() => run(() => removeAddress.mutate({ mailboxId: mailbox.id, addressId: address.id }, { onError: failed('The alias could not be removed.') }))}
              />
            ) : null}
          </View>
        ))}
        <AddressFields label={`New alias of ${mailbox.name}`} localPart={aliasLocal} domainId={aliasDomain} domains={domains} onLocalPart={setAliasLocal} onDomain={setAliasDomain} />
        <View className="flex-row">
          <Button label="Add alias" variant="outline" disabled={!aliasLocal || !aliasDomain || addAlias.isPending} onPress={submitAlias} />
        </View>
      </View>

      <View accessibilityLabel="Members" className="gap-2">
        <Text className="font-noto-medium text-[12px] text-muted-foreground">{mailbox.kind === 'personal' ? 'Owner' : 'Members'}</Text>
        {mailbox.kind === 'shared' ? (
          <MemberPicker people={pickerPeople(members)} selectedIds={readerIds} onChange={changeReaders} />
        ) : (
          <Text className="text-[13px] leading-[19px]">
            {ownerLeft
              ? 'The owner left the workspace. The mailbox refuses mail and nobody reads it; delete it when it is no longer needed.'
              : owner
                ? `${owner.display_name} (${owner.email}). Only the owner reads a personal mailbox.`
                : 'A former member.'}
          </Text>
        )}
      </View>

      {notice ? <Note error={notice.error}>{notice.text}</Note> : null}

      <View className="gap-2">
        <Text className="text-[12px] leading-[16px] text-muted-foreground">
          {mailbox.status === 'active' ? 'A suspended mailbox refuses new mail; its threads stay readable.' : 'Suspended: new mail is refused.'}
        </Text>
        <View className="flex-row flex-wrap items-center gap-2">
          {ownerLeft ? null : (
            <Button
              label={mailbox.status === 'active' ? 'Suspend' : 'Resume'}
              variant="outline"
              disabled={setStatus.isPending}
              onPress={() =>
                run(() =>
                  setStatus.mutate(
                    { mailboxId: mailbox.id, status: mailbox.status === 'active' ? 'suspended' : 'active' },
                    { onSuccess: (updated) => setNotice({ text: updated.status === 'active' ? 'Mailbox resumed' : 'Mailbox suspended' }), onError: failed('The mailbox could not be changed.') },
                  ),
                )
              }
            />
          )}
          <Button label="Delete mailbox" variant="destructive" disabled={remove.isPending} onPress={deleteMailbox} />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </View>
  )
}
