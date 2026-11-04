import { useState } from 'react'
import { Pressable, Share, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { ActionSheet, ConfirmDialog, type ConfirmRequest, type SheetAction } from '../../../components/settings/Dialog'
import { ProfilePicture } from '../../../components/settings/ProfilePicture'
import { Badge, FieldLabel, Note, RowText, Segmented, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { filterMembers, type InviteRole, invitationProblem, type MemberAction, memberActions, roleLabel } from '../../../data/account-api'
import { shortDate } from '../../../data/format'
import type { MemberRecord } from '../../../data/orbit'
import { useAuthOptions, useInvitations, useMemberWrites } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { useConversations, useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Button, Icon, IconButton, Input, Text } from '../../../ui'

const INVITE_ROLES = [
  { value: 'member', label: 'Member' },
  { value: 'admin', label: 'Admin' },
  { value: 'guest', label: 'Guest' },
] as const

/** The web's Members page: search, the members with their roles and Manage menu, then invitations. */
export default function MembersScreen() {
  const { workspace, user } = useOrbit()
  const { colors } = useTheme()
  const members = useMembers()
  const canManage = workspace.permissions.includes('members.manage')
  const writes = useMemberWrites()
  const [search, setSearch] = useState('')
  const [managing, setManaging] = useState<MemberRecord | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const list = filterMembers(members.data ?? [], search)
  const writeError = [writes.changeRole, writes.remove, writes.transfer].find((write) => write.isError)?.error

  const run = (member: MemberRecord, action: MemberAction) => {
    if (action.kind === 'role') writes.changeRole.mutate({ member, role: action.role })
    else if (action.kind === 'transfer')
      setConfirm({ title: `Transfer ownership of ${workspace.name} to ${member.display_name}?`, confirmLabel: 'Transfer ownership', danger: true, onConfirm: () => writes.transfer.mutate(member) })
    else setConfirm({ title: `Remove ${member.display_name}?`, description: 'They lose access to this workspace.', confirmLabel: 'Remove member', danger: true, onConfirm: () => writes.remove.mutate(member) })
  }
  const sheet: SheetAction[] = managing
    ? memberActions(managing, user.id).map((action) => ({
        label: action.kind === 'role' ? (managing.role === 'guest' ? 'Convert to member' : `Make ${action.role}`) : action.kind === 'transfer' ? 'Transfer ownership' : 'Remove member',
        icon: action.kind === 'role' ? 'userEdit' : action.kind === 'transfer' ? 'crown' : 'trash',
        destructive: action.kind === 'remove',
        onPress: () => run(managing, action),
      }))
    : []

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} refreshing={members.isRefetching} onRefresh={() => void members.refetch()}>
      <SettingsCard title="Members" flush>
        <View className="p-3 border-b border-border">
          <View className="h-8 flex-row items-center gap-2 px-2.5 rounded-lg border border-input">
            <Icon name="search" size={14} color={alpha(colors.mutedForeground, 0.7)} />
            <TextInput
              accessibilityLabel="Search members"
              placeholder="Search members"
              placeholderTextColor={colors.mutedForeground}
              value={search}
              onChangeText={setSearch}
              autoCapitalize="none"
              autoCorrect={false}
              className="font-noto text-[14px] leading-[1.45] flex-1 py-0 text-foreground"
            />
            {search ? <IconButton icon="close" label="Clear search" size={22} iconSize={12} color={colors.mutedForeground} onPress={() => setSearch('')} /> : null}
          </View>
        </View>
        {members.isPending ? <View className="p-4"><Note>Loading workspace members…</Note></View> : null}
        {members.isError ? <View className="p-4"><Note error>Workspace members could not be loaded.</Note></View> : null}
        {members.data && list.length === 0 ? <View className="p-4"><Note>No matching members. Try a different name, email address, or role.</Note></View> : null}
        {list.map((member, index) => {
          const actions = memberActions(member, user.id)
          return (
            <SettingsRow key={member.id} first={index === 0}>
              <ProfilePicture id={member.user_id} name={member.display_name} url={member.avatar_url} size={28} />
              <RowText
                title={member.display_name}
                subtitle={member.email}
                badges={
                  <>
                    {member.user_id === user.id ? <Badge label="You" /> : null}
                    {member.suspended_at ? <Badge label="Suspended" tone="destructive" /> : null}
                  </>
                }
              />
              <Badge label={roleLabel(member.role)} tone="secondary" />
              {actions.length > 0 ? <Button label="Manage" size="sm" variant="outline" onPress={() => setManaging(member)} /> : null}
            </SettingsRow>
          )
        })}
        {writeError ? <View className="p-4"><Note error>{errorMessage(writeError)}</Note></View> : null}
      </SettingsCard>

      {canManage ? <Invite /> : null}
      {canManage ? <PendingInvitations /> : null}

      <ActionSheet visible={managing !== null} title={managing?.display_name} actions={sheet} onClose={() => setManaging(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsShell>
  )
}

function Invite() {
  const { invite } = useMemberWrites()
  const emailEnabled = useAuthOptions().data?.email_enabled ?? false
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<InviteRole>('member')
  const [channelIds, setChannelIds] = useState<string[]>([])
  const [problem, setProblem] = useState<string | null>(null)
  const [link, setLink] = useState<string | null>(null)
  const [sent, setSent] = useState<string | null>(null)
  const send = (delivery: 'manual' | 'smtp') => {
    const address = email.trim()
    if (!address) return
    setSent(null)
    const reason = invitationProblem(role, channelIds)
    setProblem(reason)
    if (reason) return
    invite.mutate(
      { email: address, role, channelIds, delivery },
      {
        onSuccess: ({ url }) => {
          if (delivery === 'manual') setLink(url)
          else {
            setLink(null)
            setEmail('')
            setSent(`Invitation sent to ${address}`)
          }
        },
      },
    )
  }
  return (
    <SettingsCard
      title="Invite a member"
      description="Create a reusable invitation link or deliver it by email."
      actions={
        <>
          <Button label="Send email" icon="bell" variant="outline" disabled={!emailEnabled || invite.isPending || !email.trim()} onPress={() => send('smtp')} />
          <Button label="Generate link" icon="plus" disabled={invite.isPending || !email.trim()} onPress={() => send('manual')} />
        </>
      }
    >
      <View className="gap-4">
        <Input label="Email address" required value={email} onChangeText={setEmail} placeholder="teammate@example.com" keyboardType="email-address" autoCapitalize="none" autoCorrect={false} autoComplete="email" />
        <Segmented label="Role" value={role} options={INVITE_ROLES} onChange={setRole} />
        {role === 'guest' ? <GuestChannels value={channelIds} onChange={setChannelIds} /> : null}
        {problem ? <Note error>{problem}</Note> : null}
        {!emailEnabled ? <Note>Email delivery is not configured. Share the invitation link instead.</Note> : null}
        {link ? (
          <View className="gap-2">
            <Input label={`Invitation link (${roleLabel(role)})`} value={link} editable={false} selectTextOnFocus />
            <View className="flex-row">
              <Button label="Share link" icon="send" variant="outline" onPress={() => void Share.share({ message: link })} />
            </View>
          </View>
        ) : null}
        {sent ? <Note>{sent}</Note> : null}
        {invite.isError ? <Note error>{errorMessage(invite.error)}</Note> : null}
      </View>
    </SettingsCard>
  )
}

/** The private channels a guest joins: the open private channels the admin is in. */
function GuestChannels({ value, onChange }: { value: readonly string[]; onChange: (ids: string[]) => void }) {
  const { colors } = useTheme()
  const conversations = useConversations()
  const channels = (conversations.data ?? [])
    .filter((conversation) => conversation.kind === 'private' && !conversation.archived)
    .sort((a, b) => a.name.localeCompare(b.name))
  return (
    <View className="gap-1.5">
      <FieldLabel>Channels</FieldLabel>
      <Note>A guest sees only these private channels and the people in them.</Note>
      {conversations.isPending ? <Note>Loading channels…</Note> : null}
      {conversations.data && channels.length === 0 ? <Note>You are in no private channel. Make one in Chat first.</Note> : null}
      {channels.map((channel) => {
        const checked = value.includes(channel.id)
        return (
          <Pressable
            key={channel.id}
            accessibilityRole="checkbox"
            accessibilityState={{ checked }}
            accessibilityLabel={channel.name}
            onPress={() => onChange(checked ? value.filter((id) => id !== channel.id) : [...value, channel.id])}
            className="h-10 flex-row items-center gap-2 rounded-md border border-border px-3 active:bg-muted"
          >
            <Icon name={checked ? 'checkCircle' : 'circle'} size={18} color={checked ? colors.primary : colors.mutedForeground} />
            <Icon name="lock" size={14} color={colors.mutedForeground} />
            <Text numberOfLines={1} className="flex-1 text-[14px]">{channel.name}</Text>
          </Pressable>
        )
      })}
    </View>
  )
}

function PendingInvitations() {
  const invitations = useInvitations(true)
  const { revokeInvitation } = useMemberWrites()
  if (invitations.isPending) return <SettingsCard title="Pending invitations"><Note>Loading invitations…</Note></SettingsCard>
  if (invitations.isError) return <SettingsCard title="Pending invitations"><Note error>Invitations could not be loaded.</Note></SettingsCard>
  if (invitations.data.length === 0) return null
  return (
    <SettingsCard title="Pending invitations" description="Invitation links that have not been accepted." flush>
      {invitations.data.map((invitation, index) => (
        <SettingsRow key={invitation.id} first={index === 0}>
          <RowText title={invitation.email} subtitle={`Expires ${shortDate(invitation.expires_at)}`} />
          <Badge label={roleLabel(invitation.role)} tone="secondary" />
          <Button label="Revoke" variant="ghost" size="sm" disabled={revokeInvitation.isPending} onPress={() => revokeInvitation.mutate(invitation.id)} />
        </SettingsRow>
      ))}
      {revokeInvitation.isError ? <View className="p-4"><Note error>Invitation revocation failed. {errorMessage(revokeInvitation.error)}</Note></View> : null}
    </SettingsCard>
  )
}
