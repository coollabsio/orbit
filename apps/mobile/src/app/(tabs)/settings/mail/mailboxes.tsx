import { useQueryClient } from '@tanstack/react-query'
import { router } from 'expo-router'
import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { BulkPersonalDialog } from '../../../../components/settings/mail/BulkPersonalDialog'
import { MailboxDetails } from '../../../../components/settings/mail/MailboxDetails'
import { MailboxForm } from '../../../../components/settings/mail/MailboxForm'
import { MailManagerPage } from '../../../../components/settings/mail/MailboxManagersOnly'
import { Badge, Note, RowIcon, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { mailSettingsKeys, type MailDomain, type MailMailbox } from '../../../../data/mail-mailboxes-api'
import { count, mailProblem } from '../../../../data/mail-settings'
import type { MemberRecord } from '../../../../data/orbit'
import { useMailboxList, useMailDomainList } from '../../../../hooks/mail-mailboxes'
import { useOrbit } from '../../../../orbit-context'
import { useMembers } from '../../../../queries'
import { Button, EmptyState, Text } from '../../../../ui'

const DOMAINS = '/settings/mail/domains'

/** The web's Settings → Mail → Mailboxes: personal and shared, their addresses and members; one for each member at once. */
export default function MailMailboxesScreen() {
  const queryClient = useQueryClient()
  const { workspaceId } = useOrbit()
  const refresh = () => Promise.all([mailSettingsKeys.mailboxes(workspaceId), mailSettingsKeys.domains(workspaceId)].map((queryKey) => queryClient.refetchQueries({ queryKey })))
  return (
    <MailManagerPage title="Mailboxes" onRefresh={refresh}>
      <MailMailboxes />
    </MailManagerPage>
  )
}

function MailMailboxes() {
  const mailboxes = useMailboxList()
  const domains = useMailDomainList()
  const members = useMembers()
  const [dialog, setDialog] = useState<'create' | 'bulk' | null>(null)
  const [openId, setOpenId] = useState<string | null>(null)
  // the web's toasts after a create
  const [notice, setNotice] = useState<string>()

  if (mailboxes.isPending || domains.isPending) {
    return (
      <SettingsCard title="Mailboxes">
        <Note loading>Loading mailboxes…</Note>
      </SettingsCard>
    )
  }
  if (mailboxes.isError || domains.isError) {
    return (
      <SettingsCard title="Mailboxes">
        <View className="flex-row items-center gap-2">
          <View className="min-w-0 flex-1">
            <Note error>{mailProblem(mailboxes.error ?? domains.error, 'Mailboxes could not be loaded.')}</Note>
          </View>
          <Button
            label="Retry"
            variant="ghost"
            onPress={() => {
              void mailboxes.refetch()
              void domains.refetch()
            }}
          />
        </View>
      </SettingsCard>
    )
  }
  if (domains.data.length === 0) {
    return <EmptyState icon="directInbox" title="Add a domain first" description="A mailbox has an address on one of the workspace's mail domains." action={<Button label="Go to domains" onPress={() => router.push(DOMAINS)} />} />
  }

  // a member who is suspended in the workspace gets no new mailbox
  const people = (members.data ?? []).filter((member) => !member.suspended_at)
  const owned = new Set(mailboxes.data.flatMap((mailbox) => (mailbox.kind === 'personal' && mailbox.owner_membership_id ? [mailbox.owner_membership_id] : [])))
  const owners = people.filter((member) => !owned.has(member.id))
  const sorted = [...mailboxes.data].sort((a, b) => a.name.localeCompare(b.name))

  return (
    <>
      <SettingsCard
        title="Mailboxes"
        description="A personal mailbox belongs to one member. The members of a shared mailbox read it together."
        actions={
          <>
            <Button label="Create a mailbox for each member" variant="outline" disabled={owners.length === 0} onPress={() => setDialog('bulk')} />
            <Button label="New mailbox" onPress={() => setDialog('create')} />
          </>
        }
        flush
      >
        {notice ? (
          <SettingsRow first>
            <Note>{notice}</Note>
          </SettingsRow>
        ) : null}
        {sorted.length === 0 ? (
          <SettingsRow first={!notice}>
            <Note>No mailboxes yet.</Note>
          </SettingsRow>
        ) : null}
        {sorted.map((mailbox, index) => (
          <MailboxRow
            key={mailbox.id}
            mailbox={mailbox}
            first={index === 0 && !notice}
            domains={domains.data}
            members={people}
            open={openId === mailbox.id}
            onToggle={() => setOpenId((current) => (current === mailbox.id ? null : mailbox.id))}
          />
        ))}
      </SettingsCard>
      {dialog === 'create' ? (
        <MailboxForm
          domains={domains.data}
          members={people}
          owners={owners}
          onClose={() => setDialog(null)}
          onCreated={(mailbox) => {
            setNotice(`${mailbox.addresses[0]?.address ?? mailbox.name} created`)
            setDialog(null)
          }}
        />
      ) : null}
      {dialog === 'bulk' ? <BulkPersonalDialog domains={domains.data} onClose={() => setDialog(null)} onCreated={setNotice} /> : null}
    </>
  )
}

function MailboxRow({ mailbox, first, domains, members, open, onToggle }: { mailbox: MailMailbox; first: boolean; domains: MailDomain[]; members: MemberRecord[]; open: boolean; onToggle: () => void }) {
  const primary = mailbox.addresses.find((address) => address.kind === 'primary') ?? mailbox.addresses[0]
  const aliases = mailbox.addresses.length - 1
  const ownerLeft = mailbox.kind === 'personal' && !mailbox.owner_membership_id
  const owner = members.find((member) => member.id === mailbox.owner_membership_id)
  const who = mailbox.kind === 'personal' ? (ownerLeft ? 'Owner left' : (owner?.display_name ?? 'Personal')) : count(mailbox.member_ids.length, 'member', 'members')
  const catchAll = domains.filter((domain) => domain.catch_all_mailbox_id === mailbox.id)
  return (
    <View className={first ? '' : 'border-t border-border'}>
      <SettingsRow first>
        <RowIcon icon="directInbox" />
        <View className="min-w-0 flex-1 gap-0.5">
          <Text numberOfLines={1} className="font-noto-medium text-[13px] text-foreground">
            {mailbox.name}
          </Text>
          <View className="flex-row flex-wrap items-center gap-1.5">
            <Badge label={mailbox.kind === 'personal' ? 'Personal' : 'Shared'} tone="outline" />
            {mailbox.status === 'suspended' ? <Badge label="Suspended" tone="destructive" /> : null}
            {catchAll.map((domain) => (
              <Pressable
                key={domain.id}
                accessibilityRole="link"
                accessibilityLabel={`Catch-all · ${domain.name}`}
                accessibilityHint={`Gets mail for addresses of ${domain.name} that no mailbox has. Change it in Domains.`}
                onPress={() => router.push(DOMAINS)}
                className="max-w-full"
              >
                <Badge label={`Catch-all · ${domain.name}`} tone="secondary" />
              </Pressable>
            ))}
          </View>
          <Text numberOfLines={1} className="text-[12px] text-muted-foreground">
            {`${primary?.address ?? 'No address'} · ${who}${aliases > 0 ? ` · ${count(aliases, 'alias', 'aliases')}` : ''}`}
          </Text>
        </View>
        <Button label={open ? 'Close' : 'Manage'} variant="ghost" onPress={onToggle} />
      </SettingsRow>
      {open ? <MailboxDetails mailbox={mailbox} domains={domains} members={members} /> : null}
    </View>
  )
}
