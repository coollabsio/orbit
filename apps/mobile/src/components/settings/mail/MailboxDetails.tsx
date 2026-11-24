import { useState } from 'react'
import { View } from 'react-native'

import type { MailboxAddress } from '@orbit/api/generated/types.gen'

import type { MailDomain, MailMailbox } from '../../../data/mail-mailboxes-api'
import { mailProblem } from '../../../data/mail-settings'
import type { MemberRecord } from '../../../data/orbit'
import { useMailboxWrites } from '../../../hooks/mail-mailboxes'
import { Button, Input, LabelTip, Text } from '../../../ui'
import { MemberPicker } from '../../chat/ChannelSheets'
import { ConfirmDialog, type ConfirmRequest, Dialog } from '../Dialog'
import { Badge, Note } from '../Shell'
import { AddressFields, pickerPeople } from './MailboxForm'

type Notice = { text: string; error?: boolean }

/** One mailbox opened from the list: name, addresses, members, suspend and delete. */
export function MailboxDetails({ mailbox, domains, members }: { mailbox: MailMailbox; domains: readonly MailDomain[]; members: readonly MemberRecord[] }) {
  const { rename, setSenderName, removeAddress, addMember, removeMember, setStatus, remove } = useMailboxWrites()
  const [name, setName] = useState(mailbox.name)
  const [addingAlias, setAddingAlias] = useState(false)
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
    run(() => rename.mutate({ mailboxId: mailbox.id, name: trimmed }, { onSuccess: said('Sender name saved'), onError: failed('The sender name could not be saved.') }))
  }

  const removeAlias = (address: MailboxAddress) =>
    setConfirm({
      title: `Remove ${address.address}?`,
      description: 'Mail to this address no longer arrives in this mailbox. Mail it already received stays.',
      confirmLabel: 'Remove',
      danger: true,
      onConfirm: () => run(() => removeAddress.mutate({ mailboxId: mailbox.id, addressId: address.id }, { onError: failed('The alias could not be removed.') })),
    })

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
          <Input label="Sender name" help="The name recipients see next to the address on mail from this mailbox, when the address has no sender name of its own." maxLength={80} value={name} onChangeText={setName} returnKeyType="done" onSubmitEditing={saveName} />
        </View>
        <Button label="Save" variant="outline" disabled={!trimmed || trimmed === mailbox.name || rename.isPending} onPress={saveName} />
      </View>

      <View accessibilityLabel="Addresses" className="gap-2">
        <LabelTip
          label="Addresses"
          text="An address can have its own sender name. Leave it empty to use the mailbox's sender name."
          className="font-noto-medium text-[12px] text-muted-foreground"
        />
        {mailbox.addresses.map((address) => (
          <AddressRow
            // a new saved name (from here or elsewhere) resets the field
            key={`${address.id}:${address.sender_name ?? ''}`}
            mailboxName={mailbox.name}
            address={address}
            saving={setSenderName.isPending}
            removing={removeAddress.isPending}
            onSave={(senderName) =>
              run(() =>
                setSenderName.mutate(
                  { mailboxId: mailbox.id, addressId: address.id, senderName },
                  { onSuccess: said('Sender name saved'), onError: failed('The sender name could not be saved.') },
                ),
              )
            }
            onRemove={() => removeAlias(address)}
          />
        ))}
        <View className="flex-row">
          <Button label="Add alias" variant="outline" size="sm" onPress={() => setAddingAlias(true)} />
        </View>
      </View>
      {addingAlias ? <AliasDialog mailbox={mailbox} domains={domains} onClose={() => setAddingAlias(false)} onAdded={said('Alias added')} /> : null}

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

/** One address: its sender name (empty: the mailbox's) and, for an alias, **Remove**. */
function AddressRow({ mailboxName, address, saving, removing, onSave, onRemove }: {
  mailboxName: string
  address: MailboxAddress
  saving: boolean
  removing: boolean
  onSave: (senderName: string | null) => void
  onRemove: () => void
}) {
  const [senderName, setSenderName] = useState(address.sender_name ?? '')
  const trimmed = senderName.trim()
  const changed = trimmed !== (address.sender_name ?? '')
  const save = () => {
    if (changed) onSave(trimmed || null)
  }
  return (
    <View className="gap-1.5">
      <View className="min-h-7 flex-row items-center gap-2">
        <Text selectable numberOfLines={1} className="min-w-0 shrink font-mono text-[12px]">
          {address.address}
        </Text>
        <Badge label={address.kind === 'primary' ? 'Primary' : 'Alias'} tone={address.kind === 'primary' ? 'secondary' : 'outline'} />
        {address.kind === 'alias' ? <Button label="Remove" variant="destructive" size="sm" disabled={removing} onPress={onRemove} /> : null}
      </View>
      <View className="flex-row items-center gap-2">
        <View className="min-w-0 flex-1">
          <Input
            accessibilityLabel={`Sender name for ${address.address}`}
            placeholder={mailboxName}
            maxLength={80}
            value={senderName}
            onChangeText={setSenderName}
            returnKeyType="done"
            onSubmitEditing={save}
          />
        </View>
        {changed ? <Button label="Save" variant="outline" disabled={saving} onPress={save} /> : null}
      </View>
    </View>
  )
}

/** **Add alias**: the address and, if it should differ from the mailbox's, its sender name. */
function AliasDialog({ mailbox, domains, onClose, onAdded }: { mailbox: MailMailbox; domains: readonly MailDomain[]; onClose: () => void; onAdded: () => void }) {
  const { addAlias } = useMailboxWrites()
  const [localPart, setLocalPart] = useState('')
  const [domainId, setDomainId] = useState(domains[0]?.id ?? '')
  const [senderName, setSenderName] = useState('')
  const ready = localPart !== '' && domainId !== ''

  const submit = () => {
    if (!ready || addAlias.isPending) return
    addAlias.mutate(
      { mailboxId: mailbox.id, domainId, localPart, senderName: senderName.trim() || null },
      {
        onSuccess: () => {
          onAdded()
          onClose()
        },
      },
    )
  }

  return (
    <Dialog
      visible
      title={`Add an alias to ${mailbox.name}`}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={addAlias.isPending ? 'Adding…' : 'Add alias'} disabled={!ready || addAlias.isPending} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <View className="gap-1.5">
          <LabelTip label="Address" text={`Mail to ${localPart || 'name'}+anything@ arrives here too.`} />
          <AddressFields label="Address" localPart={localPart} domainId={domainId} domains={domains} onLocalPart={setLocalPart} onDomain={setDomainId} />
        </View>
        <Input
          label="Sender name"
          help="The name recipients see on mail from this alias. Leave it empty to use the mailbox's sender name."
          placeholder={mailbox.name}
          maxLength={80}
          value={senderName}
          onChangeText={setSenderName}
        />
        {addAlias.isError ? <Text className="text-[13px] text-destructive">{mailProblem(addAlias.error, 'The alias was not added.')}</Text> : null}
      </View>
    </Dialog>
  )
}
