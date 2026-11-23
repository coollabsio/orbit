import type { MailboxKind } from '@orbit/api/generated/types.gen'
import { useState } from 'react'
import { Pressable, View } from 'react-native'

import type { MailDomain, MailMailbox } from '../../../data/mail-mailboxes-api'
import { mailProblem } from '../../../data/mail-settings'
import type { MemberRecord } from '../../../data/orbit'
import { useMailboxWrites } from '../../../hooks/mail-mailboxes'
import { useTheme } from '../../../theme'
import { Button, Icon, Input, LabelTip, PickerSheet, Text } from '../../../ui'
import { MemberPicker } from '../../chat/ChannelSheets'
import { Dialog } from '../Dialog'
import { FieldLabel, Note, Select } from '../Shell'

const KIND_OPTIONS: readonly { value: MailboxKind; label: string }[] = [
  { value: 'shared', label: 'Shared: the members you choose read it' },
  { value: 'personal', label: 'Personal: only its owner reads it' },
]

/** `MemberPicker` takes people by user id, as the web's does. */
export const pickerPeople = (members: readonly MemberRecord[]) => members.map((member) => ({ id: member.user_id, name: member.display_name }))

/** The address field: the local part, `@` and the domain. */
export function AddressFields({ label, localPart, domainId, domains, onLocalPart, onDomain }: { label: string; localPart: string; domainId: string; domains: readonly MailDomain[]; onLocalPart: (value: string) => void; onDomain: (value: string) => void }) {
  return (
    <View className="flex-row flex-wrap items-center gap-1.5">
      <View className="min-w-[120px] flex-1">
        <Input
          accessibilityLabel={label}
          autoComplete="off"
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="email-address"
          placeholder="team"
          value={localPart}
          onChangeText={(value) => onLocalPart(value.trim().toLowerCase())}
        />
      </View>
      <Text className="text-muted-foreground">@</Text>
      <View className="min-w-[120px] flex-1">
        <Select label="Domain" value={domainId} options={domains.map((domain) => ({ value: domain.id, label: domain.name }))} onChange={onDomain} />
      </View>
    </View>
  )
}

/** The web `Select` with a placeholder while nothing is chosen. */
function ChoiceField({ label, value, placeholder, options, onChange }: { label: string; value: string; placeholder: string; options: readonly { value: string; label: string }[]; onChange: (value: string) => void }) {
  const { colors } = useTheme()
  const [open, setOpen] = useState(false)
  const current = options.find((option) => option.value === value)
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={label}
        onPress={() => setOpen(true)}
        className="h-8 flex-row items-center justify-between gap-1.5 rounded-lg border border-input pr-2 pl-2.5 active:bg-input/50 dark:bg-input/30"
      >
        <Text numberOfLines={1} className={`shrink text-[14px] ${current ? 'text-foreground' : 'text-muted-foreground'}`}>
          {current?.label ?? placeholder}
        </Text>
        <Icon name="chevronDown" size={16} color={colors.mutedForeground} />
      </Pressable>
      <PickerSheet
        visible={open}
        title={label}
        options={options.map((option) => ({ key: option.value, label: option.label, selected: option.value === value }))}
        onSelect={(key) => {
          setOpen(false)
          onChange(key)
        }}
        onClose={() => setOpen(false)}
      />
    </>
  )
}

/**
 * **New mailbox**: personal (one owner, who has no personal mailbox yet) or shared (the chosen members read it).
 * `owners` are the members without a personal mailbox.
 */
export function MailboxForm({ domains, members, owners, onClose, onCreated }: { domains: readonly MailDomain[]; members: readonly MemberRecord[]; owners: readonly MemberRecord[]; onClose: () => void; onCreated: (mailbox: MailMailbox) => void }) {
  const { create } = useMailboxWrites()
  const [kind, setKind] = useState<MailboxKind>('shared')
  const [name, setName] = useState('')
  const [localPart, setLocalPart] = useState('')
  const [domainId, setDomainId] = useState(domains[0]?.id ?? '')
  const [ownerId, setOwnerId] = useState('')
  /** User ids, as `MemberPicker` takes them. */
  const [readerIds, setReaderIds] = useState<string[]>([])
  const ready = name.trim() !== '' && localPart !== '' && domainId !== '' && (kind === 'shared' || ownerId !== '')

  const submit = () => {
    if (!ready || create.isPending) return
    const memberIds = readerIds.flatMap((userId) => members.find((member) => member.user_id === userId)?.id ?? [])
    create.mutate(
      { kind, name: name.trim(), domain_id: domainId, local_part: localPart, owner_membership_id: kind === 'personal' ? ownerId : null, member_ids: kind === 'shared' ? memberIds : [] },
      { onSuccess: onCreated },
    )
  }

  return (
    <Dialog
      visible
      title="New mailbox"
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={create.isPending ? 'Creating…' : 'Create mailbox'} disabled={!ready || create.isPending} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <View className="gap-1.5">
          <FieldLabel>Kind</FieldLabel>
          <Select label="Kind" value={kind} options={KIND_OPTIONS} onChange={setKind} />
        </View>
        {kind === 'personal' ? (
          <View className="gap-1.5">
            <LabelTip label="Owner" text="A member has one personal mailbox. Add more addresses to it as aliases." />
            <ChoiceField
              label="Owner"
              value={ownerId}
              placeholder={owners.length ? 'Choose a member' : 'Every member has a personal mailbox'}
              options={owners.map((member) => ({ value: member.id, label: `${member.display_name} (${member.email})` }))}
              onChange={(value) => {
                const owner = owners.find((member) => member.id === value)
                setOwnerId(value)
                if (owner && !name.trim()) setName(owner.display_name)
              }}
            />
          </View>
        ) : null}
        <Input label="Name" maxLength={80} placeholder={kind === 'shared' ? 'Support' : 'Ann Lee'} value={name} onChangeText={setName} />
        <View className="gap-1.5">
          <LabelTip label="Address" text={`Mail to ${localPart || 'name'}+anything@ arrives here too.`} />
          <AddressFields label="Address" localPart={localPart} domainId={domainId} domains={domains} onLocalPart={setLocalPart} onDomain={setDomainId} />
        </View>
        {kind === 'shared' ? (
          <View className="gap-1.5">
            <LabelTip label="Members" text="They read its mail. Owners and admins manage the mailbox; they read it only as members." />
            <MemberPicker people={pickerPeople(members)} selectedIds={readerIds} onChange={setReaderIds} />
          </View>
        ) : null}
        {create.isError ? <Note error>{mailProblem(create.error, 'The mailbox was not created.')}</Note> : null}
      </View>
    </Dialog>
  )
}
