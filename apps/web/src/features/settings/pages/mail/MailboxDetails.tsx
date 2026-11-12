import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import type { MailDomain, MailMailbox } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
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
} from '@/features/mail/api/mutations'
import type { User } from '@/features/workspaces/models'
import { AddressFields } from './MailboxForm'
import { mailProblem } from './mailSettingsLib'

const failed = (fallback: string) => (error: unknown) => toast.error(mailProblem(error, fallback))

/** One mailbox opened from the list: name, addresses, members, suspend and delete. */
export function MailboxDetails({ mailbox, domains, members }: { mailbox: MailMailbox; domains: MailDomain[]; members: User[] }) {
  const ids = useId()
  const rename = useUpdateMailbox()
  const addAlias = useAddMailboxAlias()
  const removeAddress = useRemoveMailboxAddress()
  const addMember = useAddMailboxMember()
  const removeMember = useRemoveMailboxMember()
  const setStatus = useSetMailboxStatus()
  const remove = useDeleteMailbox()
  const [name, setName] = useState(mailbox.name)
  const [aliasLocal, setAliasLocal] = useState('')
  const [aliasDomain, setAliasDomain] = useState(domains[0]?.id ?? '')
  const ownerLeft = mailbox.kind === 'personal' && !mailbox.owner_membership_id
  const owner = members.find((member) => member.membershipId === mailbox.owner_membership_id)
  const readerIds = mailbox.member_ids.flatMap((membershipId) => members.find((member) => member.membershipId === membershipId)?.id ?? [])

  function saveName(event: FormEvent) {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || trimmed === mailbox.name) return
    rename.mutate({ mailboxId: mailbox.id, name: trimmed }, { onSuccess: () => toast('Mailbox renamed'), onError: failed('The mailbox could not be renamed.') })
  }

  function submitAlias(event: FormEvent) {
    event.preventDefault()
    if (!aliasLocal || !aliasDomain) return
    addAlias.mutate({ mailboxId: mailbox.id, local_part: aliasLocal, domain_id: aliasDomain }, {
      onSuccess: () => setAliasLocal(''),
      onError: failed('The alias could not be added.'),
    })
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
          <FieldLabel htmlFor={`${ids}-name`}>Name</FieldLabel>
          <Input id={`${ids}-name`} maxLength={80} value={name} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Button type="submit" variant="outline" disabled={!name.trim() || name.trim() === mailbox.name || rename.isPending}>Rename</Button>
      </form>

      <section aria-label="Addresses" className="flex flex-col gap-2">
        <h4 className="text-xs font-medium text-muted-foreground">Addresses</h4>
        <ul className="flex flex-col gap-1">
          {mailbox.addresses.map((address) => (
            <li key={address.id} className="flex items-center gap-2 text-[0.8125rem]">
              <span className="min-w-0 truncate font-mono text-xs">{address.address}</span>
              <Badge variant={address.kind === 'primary' ? 'secondary' : 'outline'}>{address.kind === 'primary' ? 'Primary' : 'Alias'}</Badge>
              {address.kind === 'alias' ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  aria-label={`Remove ${address.address}`}
                  disabled={removeAddress.isPending}
                  onClick={() => removeAddress.mutate({ mailboxId: mailbox.id, addressId: address.id }, { onError: failed('The alias could not be removed.') })}
                >
                  Remove
                </Button>
              ) : null}
            </li>
          ))}
        </ul>
        <form className="flex flex-wrap items-center gap-2" onSubmit={submitAlias}>
          <AddressFields id={`${ids}-alias`} localPart={aliasLocal} domainId={aliasDomain} domains={domains} onLocalPart={setAliasLocal} onDomain={setAliasDomain} />
          <Button type="submit" variant="outline" disabled={!aliasLocal || !aliasDomain || addAlias.isPending}>Add alias</Button>
        </form>
      </section>

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
