import { useState } from 'react'
import { Link } from 'react-router'
import { DirectInbox } from 'reicon-react'
import type { MailDomain, MailMailbox } from '@orbit/api/generated/types.gen'
import { EmptyState } from '@/components/common/EmptyState'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { useMailboxes, useMailDomains } from '@/features/mail/api/queries'
import { RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { BulkPersonalDialog } from './BulkPersonalDialog'
import { MailboxDetails } from './MailboxDetails'
import { MailboxForm } from './MailboxForm'
import { MailManagersOnly } from './MailDomainsPage'
import { count, mailProblem } from './mailSettingsLib'

/** The workspace's mailboxes: personal and shared, their addresses and members; one for each member at once. */
export function MailMailboxesPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Mailboxes" />
  return <MailMailboxes />
}

function MailMailboxes() {
  const { workspace } = useWorkspace()
  const mailboxes = useMailboxes()
  const domains = useMailDomains()
  const members = useMembers(workspace.id)
  const [dialog, setDialog] = useState<'create' | 'bulk' | null>(null)
  const [openId, setOpenId] = useState<string | null>(null)

  if (mailboxes.isPending || domains.isPending) return <SettingsCard title="Mailboxes"><p role="status">Loading mailboxes…</p></SettingsCard>
  if (mailboxes.isError || domains.isError) {
    return (
      <SettingsCard title="Mailboxes">
        <p role="alert">
          {mailProblem(mailboxes.error ?? domains.error, 'Mailboxes could not be loaded.')}{' '}
          <Button variant="ghost" onClick={() => { void mailboxes.refetch(); void domains.refetch() }}>Retry</Button>
        </p>
      </SettingsCard>
    )
  }
  if (domains.data.length === 0) {
    return (
      <EmptyState
        icon={DirectInbox}
        title="Add a domain first"
        description="A mailbox has an address on one of the workspace's mail domains."
        action={<Link className={buttonVariants()} to="/settings/mail/domains">Go to domains</Link>}
      />
    )
  }

  // a member who is suspended in the workspace gets no new mailbox
  const people = (members.data ?? []).filter((member) => !member.suspended)
  const owned = new Set(mailboxes.data.flatMap((mailbox) => (mailbox.kind === 'personal' && mailbox.owner_membership_id ? [mailbox.owner_membership_id] : [])))
  const owners = people.filter((member) => !owned.has(member.membershipId))
  const sorted = [...mailboxes.data].sort((a, b) => a.name.localeCompare(b.name))

  return (
    <>
      <SettingsCard
        title="Mailboxes"
        description="A personal mailbox belongs to one member. The members of a shared mailbox read it together."
        actions={(
          <>
            <Button type="button" variant="outline" disabled={owners.length === 0} onClick={() => setDialog('bulk')}>Create a mailbox for each member</Button>
            <Button type="button" onClick={() => setDialog('create')}>New mailbox</Button>
          </>
        )}
        flush
      >
        <div className="flex flex-col divide-y">
          {sorted.length === 0 ? <SettingsRow className="text-muted-foreground">No mailboxes yet.</SettingsRow> : null}
          {sorted.map((mailbox) => (
            <MailboxRow
              key={mailbox.id}
              mailbox={mailbox}
              domains={domains.data}
              members={people}
              open={openId === mailbox.id}
              onToggle={() => setOpenId((current) => (current === mailbox.id ? null : mailbox.id))}
            />
          ))}
        </div>
      </SettingsCard>
      {dialog === 'create' ? (
        <Modal title="New mailbox" onClose={() => setDialog(null)} className="sm:max-w-lg">
          <MailboxForm domains={domains.data} members={people} owners={owners} onDone={() => setDialog(null)} />
        </Modal>
      ) : null}
      {dialog === 'bulk' ? <BulkPersonalDialog domains={domains.data} onClose={() => setDialog(null)} /> : null}
    </>
  )
}

function MailboxRow({ mailbox, domains, members, open, onToggle }: { mailbox: MailMailbox; domains: MailDomain[]; members: User[]; open: boolean; onToggle: () => void }) {
  const primary = mailbox.addresses.find((address) => address.kind === 'primary') ?? mailbox.addresses[0]
  const aliases = mailbox.addresses.length - 1
  const ownerLeft = mailbox.kind === 'personal' && !mailbox.owner_membership_id
  const owner = members.find((member) => member.membershipId === mailbox.owner_membership_id)
  const who = mailbox.kind === 'personal'
    ? (ownerLeft ? 'Owner left' : owner?.name ?? 'Personal')
    : count(mailbox.member_ids.length, 'member', 'members')
  return (
    <div>
      <SettingsRow className="flex-nowrap">
        <RowIcon><DirectInbox /></RowIcon>
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="flex min-w-0 items-center gap-2">
            <span className="truncate text-[13px] font-medium text-foreground">{mailbox.name}</span>
            <Badge variant="outline">{mailbox.kind === 'personal' ? 'Personal' : 'Shared'}</Badge>
            {mailbox.status === 'suspended' ? <Badge variant="destructive">Suspended</Badge> : null}
          </span>
          <span className="truncate text-xs text-muted-foreground">
            {primary?.address ?? 'No address'} · {who}{aliases > 0 ? ` · ${count(aliases, 'alias', 'aliases')}` : ''}
          </span>
        </div>
        <Button type="button" variant="ghost" aria-expanded={open} aria-label={`Manage ${mailbox.name}`} onClick={onToggle}>
          {open ? 'Close' : 'Manage'}
        </Button>
      </SettingsRow>
      {open ? <MailboxDetails mailbox={mailbox} domains={domains} members={members} /> : null}
    </div>
  )
}
