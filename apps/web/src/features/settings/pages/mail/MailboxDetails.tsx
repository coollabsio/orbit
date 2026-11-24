import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import type { MailboxAddress, MailDomain, MailMailbox } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { Modal } from '@/components/common/Modal'
import { InfoTip } from '@/components/common/InfoTip'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { MemberPicker } from '@/features/chat/components/dialogs/MemberPicker'
import {
  useAddMailboxAlias,
  useAddMailboxMember,
  useDeleteMailbox,
  useRemoveMailboxAddress,
  useRemoveMailboxMember,
  useSetMailboxStatus,
  useUpdateMailbox,
  useUpdateMailboxAddress,
} from '@/features/mail/api/mutations'
import type { User } from '@/features/workspaces/models'
import { AddressFields } from './MailboxForm'
import { mailProblem } from './mailSettingsLib'
import { VacationForm } from './VacationForm'

const failed = (fallback: string) => (error: unknown) => toast.error(mailProblem(error, fallback))

/** One mailbox opened from the list: name, addresses, members, a shared one's vacation reply, suspend and delete. */
export function MailboxDetails({ mailbox, domains, members }: { mailbox: MailMailbox; domains: MailDomain[]; members: User[] }) {
  const ids = useId()
  const rename = useUpdateMailbox()
  const removeAddress = useRemoveMailboxAddress()
  const addMember = useAddMailboxMember()
  const removeMember = useRemoveMailboxMember()
  const setStatus = useSetMailboxStatus()
  const remove = useDeleteMailbox()
  const [name, setName] = useState(mailbox.name)
  const [addingAlias, setAddingAlias] = useState(false)
  const ownerLeft = mailbox.kind === 'personal' && !mailbox.owner_membership_id
  const owner = members.find((member) => member.membershipId === mailbox.owner_membership_id)
  const readerIds = mailbox.member_ids.flatMap((membershipId) => members.find((member) => member.membershipId === membershipId)?.id ?? [])

  function saveName(event: FormEvent) {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || trimmed === mailbox.name) return
    rename.mutate({ mailboxId: mailbox.id, name: trimmed }, { onSuccess: () => toast('Sender name saved'), onError: failed('The sender name could not be saved.') })
  }

  async function removeAlias(address: MailboxAddress) {
    const confirmed = await confirmAction({
      title: `Remove ${address.address}?`,
      description: 'Mail to this address no longer arrives in this mailbox. Mail it already received stays.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (confirmed) removeAddress.mutate({ mailboxId: mailbox.id, addressId: address.id }, { onError: failed('The alias could not be removed.') })
  }

  /** `MemberPicker` reports the whole new list; one person changes at a time. */
  function changeReaders(next: string[]) {
    const added = next.find((userId) => !readerIds.includes(userId))
    const removed = readerIds.find((userId) => !next.includes(userId))
    const membershipOf = (userId: string) => members.find((member) => member.id === userId)?.membershipId
    const addedMembership = added ? membershipOf(added) : undefined
    const removedMembership = removed ? membershipOf(removed) : undefined
    if (addedMembership) addMember.mutate({ mailboxId: mailbox.id, membershipId: addedMembership }, { onError: failed('The member could not be added.') })
    if (removedMembership) removeMember.mutate({ mailboxId: mailbox.id, membershipId: removedMembership }, { onError: failed('The member could not be removed.') })
  }

  async function deleteMailbox() {
    const confirmed = await confirmAction({
      title: `Delete ${mailbox.name}?`,
      description: 'Its addresses stop receiving mail and its threads are deleted for every member. You cannot undo this.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (confirmed) remove.mutate(mailbox.id, { onSuccess: () => toast(`${mailbox.name} deleted`), onError: failed('The mailbox could not be deleted.') })
  }

  return (
    <div className="flex flex-col gap-5 border-t bg-muted/30 px-4 py-4">
      <form className="flex flex-wrap items-end gap-2" onSubmit={saveName}>
        <Field className="w-64">
          <FieldLabel htmlFor={`${ids}-name`}>Sender name <InfoTip text="The name recipients see next to the address on mail from this mailbox, when the address has no sender name of its own." /></FieldLabel>
          <Input id={`${ids}-name`} maxLength={80} value={name} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Button type="submit" variant="outline" disabled={!name.trim() || name.trim() === mailbox.name || rename.isPending}>Save</Button>
      </form>

      <section aria-label="Addresses" className="flex flex-col gap-2">
        <h4 className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
          Addresses <InfoTip text="An address can have its own sender name. Leave it empty to use the mailbox's sender name." />
        </h4>
        {/* one grid for all rows, so the sender name fields and the buttons line up */}
        <ul className="grid grid-cols-[max-content_minmax(0,16rem)_max-content] items-center gap-x-3 gap-y-2">
          {mailbox.addresses.map((address) => (
            <AddressRow
              // a new saved name (from here or elsewhere) resets the field
              key={`${address.id}:${address.sender_name ?? ''}`}
              mailboxId={mailbox.id}
              mailboxName={mailbox.name}
              address={address}
              removing={removeAddress.isPending}
              onRemove={() => void removeAlias(address)}
            />
          ))}
        </ul>
        <div>
          <Button type="button" variant="outline" size="sm" onClick={() => setAddingAlias(true)}>Add alias</Button>
        </div>
      </section>
      {addingAlias ? <AliasDialog mailbox={mailbox} domains={domains} onClose={() => setAddingAlias(false)} /> : null}

      <section aria-label="Members" className="flex flex-col gap-2">
        <h4 className="text-xs font-medium text-muted-foreground">{mailbox.kind === 'personal' ? 'Owner' : 'Members'}</h4>
        {mailbox.kind === 'shared' ? (
          <div className="max-w-md">
            <MemberPicker label={`Members of ${mailbox.name}`} people={members} selectedIds={readerIds} onChange={changeReaders} />
          </div>
        ) : (
          <p className="m-0 text-[0.8125rem]">
            {ownerLeft ? 'The owner left the workspace. The mailbox refuses mail and nobody reads it; delete it when it is no longer needed.' : (owner ? `${owner.name} (${owner.email}). Only the owner reads a personal mailbox.` : 'A former member.')}
          </p>
        )}
      </section>

      {mailbox.kind === 'shared' ? (
        <section aria-label="Vacation reply" className="flex max-w-2xl flex-col gap-2">
          <VacationForm mailboxId={mailbox.id} mailboxName={mailbox.name} />
        </section>
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        {ownerLeft ? null : (
          <Button
            type="button"
            variant="outline"
            disabled={setStatus.isPending}
            onClick={() => setStatus.mutate({ mailboxId: mailbox.id, status: mailbox.status === 'active' ? 'suspended' : 'active' }, {
              onSuccess: (updated) => toast(updated.status === 'active' ? 'Mailbox resumed' : 'Mailbox suspended'),
              onError: failed('The mailbox could not be changed.'),
            })}
          >
            {mailbox.status === 'active' ? 'Suspend' : 'Resume'}
          </Button>
        )}
        <span className="flex-1 text-xs text-muted-foreground">
          {mailbox.status === 'active' ? 'A suspended mailbox refuses new mail; its threads stay readable.' : 'Suspended: new mail is refused.'}
        </span>
        <Button type="button" variant="ghost" className="text-destructive" disabled={remove.isPending} onClick={() => void deleteMailbox()}>Delete mailbox</Button>
      </div>
    </div>
  )
}

/** One address: its sender name (empty: the mailbox's) and, for an alias, **Remove**. */
function AddressRow({ mailboxId, mailboxName, address, removing, onRemove }: {
  mailboxId: string
  mailboxName: string
  address: MailboxAddress
  removing: boolean
  onRemove: () => void
}) {
  const update = useUpdateMailboxAddress()
  const [senderName, setSenderName] = useState(address.sender_name ?? '')
  const changed = senderName.trim() !== (address.sender_name ?? '')

  function save(event: FormEvent) {
    event.preventDefault()
    if (!changed) return
    update.mutate(
      { mailboxId, addressId: address.id, senderName: senderName.trim() || null },
      { onSuccess: () => toast('Sender name saved'), onError: failed('The sender name could not be saved.') },
    )
  }

  return (
    <li className="col-span-3 grid grid-cols-subgrid items-center">
      <form className="contents" onSubmit={save}>
        <span className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 truncate font-mono text-xs">{address.address}</span>
          <Badge variant={address.kind === 'primary' ? 'secondary' : 'outline'}>{address.kind === 'primary' ? 'Primary' : 'Alias'}</Badge>
        </span>
        <Input
          className="h-7 text-xs"
          maxLength={80}
          aria-label={`Sender name for ${address.address}`}
          placeholder={mailboxName}
          value={senderName}
          onChange={(event) => setSenderName(event.target.value)}
        />
        <span className="flex items-center gap-1">
          {changed ? <Button type="submit" variant="outline" size="sm" disabled={update.isPending}>Save</Button> : null}
          {address.kind === 'alias' ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
              aria-label={`Remove ${address.address}`}
              disabled={removing}
              onClick={onRemove}
            >
              Remove
            </Button>
          ) : null}
        </span>
      </form>
    </li>
  )
}

/** **Add alias**: the address and, if it should differ from the mailbox's, its sender name. */
function AliasDialog({ mailbox, domains, onClose }: { mailbox: MailMailbox; domains: MailDomain[]; onClose: () => void }) {
  const ids = useId()
  const addAlias = useAddMailboxAlias()
  const [localPart, setLocalPart] = useState('')
  const [domainId, setDomainId] = useState(domains[0]?.id ?? '')
  const [senderName, setSenderName] = useState('')

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!localPart || !domainId) return
    addAlias.mutate(
      { mailboxId: mailbox.id, local_part: localPart, domain_id: domainId, sender_name: senderName.trim() || null },
      { onSuccess: () => { toast('Alias added'); onClose() } },
    )
  }

  return (
    <Modal title={`Add an alias to ${mailbox.name}`} onClose={onClose} className="sm:max-w-lg" dismissible={!addAlias.isPending}>
      <form className="grid gap-4" onSubmit={submit}>
        <Field>
          <FieldLabel htmlFor={`${ids}-local`}>Address <InfoTip text={`Mail to ${localPart || 'name'}+anything@ arrives here too.`} /></FieldLabel>
          <AddressFields id={`${ids}-local`} localPart={localPart} domainId={domainId} domains={domains} onLocalPart={setLocalPart} onDomain={setDomainId} />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${ids}-sender`}>Sender name <InfoTip text="The name recipients see on mail from this alias. Leave it empty to use the mailbox's sender name." /></FieldLabel>
          <Input id={`${ids}-sender`} maxLength={80} placeholder={mailbox.name} value={senderName} onChange={(event) => setSenderName(event.target.value)} />
        </Field>
        {addAlias.isError ? <p role="alert" className="text-sm text-destructive">{mailProblem(addAlias.error, 'The alias was not added.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={!localPart || !domainId || addAlias.isPending}>{addAlias.isPending ? 'Adding…' : 'Add alias'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
