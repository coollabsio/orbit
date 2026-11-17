import { useState } from 'react'
import { Pressable, Share, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { ListPager, usePagedList } from '../../../components/settings/ListPager'
import { ActionSheet, ConfirmDialog, type ConfirmRequest, type SheetAction } from '../../../components/settings/Dialog'
import { Badge, FieldLabel, InitialAvatar, Note, RowText, Select, SettingsCard, SettingsRow, SettingsShell, SmallOutlineButton, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { filterMembers, type InviteRole, invitationProblem, type MemberAction, memberActions, roleLabel } from '../../../data/account-api'
import { shortDate } from '../../../data/format'
import type { MemberRecord } from '../../../data/orbit'
import { useAuthOptions, useInvitations, useMemberWrites, useWorkspaceRoles } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { useConversations, useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Button, Icon, IconButton, Input, Text, LabelTip } from '../../../ui'

const INVITE_ROLES = [
  { value: 'admin', label: 'Admin' },
  { value: 'member', label: 'Member' },
  { value: 'guest', label: 'Guest' },
] as const

type Sort = 'name_asc' | 'name_desc' | 'email_asc' | 'role'
const ROLE_FILTERS = [{ value: 'all', label: 'All roles' }, ...['owner', 'admin', 'member', 'guest'].map((role) => ({ value: role, label: roleLabel(role) }))]
const SORTS: { value: Sort; label: string }[] = [
  { value: 'name_asc', label: 'Name A–Z' },
  { value: 'name_desc', label: 'Name Z–A' },
  { value: 'email_asc', label: 'Email A–Z' },
  { value: 'role', label: 'Role' },
]

/** The web's role filter and sort over the searched members. */
function sortedMembers(members: MemberRecord[], role: string, sort: Sort): MemberRecord[] {
  const list = role === 'all' ? members : members.filter((member) => member.role === role)
  if (sort === 'name_desc') return [...list].sort((a, b) => b.display_name.localeCompare(a.display_name))
  if (sort === 'email_asc') return [...list].sort((a, b) => a.email.localeCompare(b.email))
  if (sort === 'role') return [...list].sort((a, b) => a.role.localeCompare(b.role))
  return list
}

/** The web's Members page: search, the members with their roles and Manage menu, then invitations. */
export default function MembersScreen() {
  const { workspace, user } = useOrbit()
  const { colors } = useTheme()
  const members = useMembers()
  const canManage = workspace.permissions.includes('members.manage')
  const canManageRoles = workspace.permissions.includes('roles.manage')
  const roles = useWorkspaceRoles(canManageRoles)
  const writes = useMemberWrites()
  const [search, setSearch] = useState('')
  const [managing, setManaging] = useState<MemberRecord | null>(null)
  const [assigning, setAssigning] = useState<MemberRecord | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [roleFilter, setRoleFilter] = useState('all')
  const [sort, setSort] = useState<Sort>('name_asc')
  const list = sortedMembers(filterMembers(members.data ?? [], search), roleFilter, sort)
  const { visible, pager, resetPage } = usePagedList(list)
  const writeError = [writes.changeRole, writes.setCustomRole, writes.remove, writes.transfer].find((write) => write.isError)?.error

  const run = (member: MemberRecord, action: MemberAction) => {
    if (action.kind === 'role') writes.changeRole.mutate({ member, role: action.role })
    else if (action.kind === 'customRole') setAssigning(member)
    else if (action.kind === 'transfer')
      setConfirm({ title: `Transfer ownership of ${workspace.name} to ${member.display_name}?`, confirmLabel: 'Transfer ownership', danger: true, onConfirm: () => writes.transfer.mutate(member) })
    else setConfirm({ title: `Remove ${member.display_name}?`, description: 'They lose access to this workspace.', confirmLabel: 'Remove member', danger: true, onConfirm: () => writes.remove.mutate(member) })
  }
  const sheet: SheetAction[] = managing
    ? memberActions(managing, user.id, canManageRoles).map((action) => ({
        label: action.kind === 'role' ? (managing.role === 'guest' ? 'Convert to member' : `Make ${action.role}`) : action.kind === 'customRole' ? 'Set custom role' : action.kind === 'transfer' ? 'Transfer ownership' : 'Remove member',
        icon: action.kind === 'role' ? 'userEdit' : action.kind === 'customRole' ? 'shieldUser' : action.kind === 'transfer' ? 'crown' : 'trash',
        destructive: action.kind === 'remove',
        onPress: () => run(managing, action),
      }))
    : []
  // the custom roles to pick from, the current one checked, then taking it away
  const roleItems = roles.data?.items ?? []
  const roleSheet: SheetAction[] = assigning
    ? [
        ...(roleItems.length === 0
          ? [{ label: roles.isPending ? 'Loading roles…' : roles.isError ? 'Roles could not be loaded.' : 'No custom roles yet', detail: roles.isPending || roles.isError ? undefined : 'Create one in Roles.', disabled: true, onPress: () => {} }]
          : []),
        ...roleItems.map((role): SheetAction => ({
          label: role.name,
          icon: assigning.custom_role?.id === role.id ? 'checkCircle' : 'circle',
          onPress: () => writes.setCustomRole.mutate({ member: assigning, roleId: role.id }),
        })),
        ...(assigning.custom_role ? [{ label: 'Remove custom role', icon: 'close', destructive: true, onPress: () => writes.setCustomRole.mutate({ member: assigning, roleId: null }) } satisfies SheetAction] : []),
      ]
    : []

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => members.refetch()}>
      <SettingsCard title="Members" flush>
        <View className="p-3 gap-2 border-b border-border">
          <View className="h-8 flex-row items-center gap-2 px-2.5 rounded-lg border border-input">
            <Icon name="search" size={14} color={alpha(colors.mutedForeground, 0.7)} />
            <TextInput
              accessibilityLabel="Search members"
              placeholder="Search members"
              placeholderTextColor={colors.mutedForeground}
              value={search}
              onChangeText={(text) => {
                setSearch(text)
                resetPage()
              }}
              autoCapitalize="none"
              autoCorrect={false}
              className="font-noto text-[14px] leading-[1.45] flex-1 py-0 text-foreground"
            />
            {search ? <IconButton icon="close" label="Clear search" size={22} iconSize={12} color={colors.mutedForeground} onPress={() => setSearch('')} /> : null}
          </View>
          <Select label="Filter by role" value={roleFilter} options={ROLE_FILTERS} onChange={(value) => { setRoleFilter(value); resetPage() }} />
          <Select label="Sort members" value={sort} options={SORTS} onChange={(value) => { setSort(value); resetPage() }} />
        </View>
        {members.isPending ? <View className="p-4"><Note loading>Loading workspace members…</Note></View> : null}
        {members.isError ? <View className="p-4"><Note error>Workspace members could not be loaded.</Note></View> : null}
        {members.data && list.length === 0 ? <View className="p-4"><Note>No matching members. Try a different name, email address, or role.</Note></View> : null}
        {visible.length > 0 ? (
          // the web's member table at phone width: Name, Role and Actions (the email column hides)
          <View className="h-10 flex-row items-center gap-4 px-4 border-b border-muted bg-foreground/[0.02]">
            <Text className="flex-1 font-noto-medium text-[13px] text-muted-foreground">Name</Text>
            <Text className="font-noto-medium text-[13px] text-muted-foreground">Role</Text>
            <Text className="font-noto-medium text-[13px] text-muted-foreground text-right">Actions</Text>
          </View>
        ) : null}
        {visible.map((member) => {
          const actions = memberActions(member, user.id, canManageRoles)
          return (
            <View key={member.id} className="min-h-12 flex-row items-center gap-4 px-4 py-2.5 border-b border-border">
              <View className="flex-1 min-w-0 flex-row items-center gap-2">
                {/* the web's initial avatar; the app knows only your own presence (the web's comes over its socket) */}
                <InitialAvatar name={member.display_name || member.email} size={28} presence={member.user_id === user.id ? user.status.presence : undefined} />
                <Text numberOfLines={1} className="shrink font-noto-medium text-[13px] text-foreground">
                  {member.display_name}
                </Text>
                {member.user_id === user.id ? <Badge label="You" /> : null}
                {member.suspended_at ? <Badge label="Suspended" tone="destructive" /> : null}
              </View>
              <View className="items-end gap-1">
                <Badge label={roleLabel(member.role)} tone="secondary" />
                {member.custom_role ? <Badge label={member.custom_role.name} tone="outline" /> : null}
              </View>
              {actions.length > 0 ? <SmallOutlineButton label="Manage" accessibilityLabel={`Manage ${member.display_name}`} onPress={() => setManaging(member)} /> : null}
            </View>
          )
        })}
        {list.length > 0 ? <ListPager {...pager} /> : null}
        {writeError ? <View className="p-4"><Note error>{errorMessage(writeError)}</Note></View> : null}
      </SettingsCard>

      {canManage ? <Invite /> : null}
      {canManage ? <PendingInvitations /> : null}

      <ActionSheet visible={managing !== null} title={managing?.display_name} actions={sheet} onClose={() => setManaging(null)} />
      <ActionSheet visible={assigning !== null} title={assigning ? `Custom role of ${assigning.display_name}` : undefined} actions={roleSheet} onClose={() => setAssigning(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsShell>
  )
}

/** The web's `outline` `icon-sm` pager button. */
function Invite() {
  const { invite } = useMemberWrites()
  const emailEnabled = useAuthOptions().data?.email_enabled ?? false
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<InviteRole>('member')
  const [channelIds, setChannelIds] = useState<string[]>([])
  const [problem, setProblem] = useState<string | null>(null)
  // The role is the one the link was issued with, not the role picked since.
  const [link, setLink] = useState<{ url: string; role: InviteRole } | null>(null)
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
          if (delivery === 'manual') setLink(url ? { url, role } : null)
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
          <Button label="Send email" icon="inbox" variant="outline" disabled={!emailEnabled || invite.isPending} onPress={() => send('smtp')} />
          <Button label="Generate link" icon="plus" disabled={invite.isPending} onPress={() => send('manual')} />
        </>
      }
    >
      <View className="gap-4">
        <Input label="Email address" required value={email} onChangeText={setEmail} placeholder="teammate@example.com" keyboardType="email-address" autoCapitalize="none" autoCorrect={false} autoComplete="email" />
        <View className="gap-2">
          <FieldLabel>Role</FieldLabel>
          <Select label="Role" value={role} options={INVITE_ROLES} onChange={setRole} />
        </View>
        {role === 'guest' ? <GuestChannels value={channelIds} onChange={setChannelIds} /> : null}
        {problem ? <Note error>{problem}</Note> : null}
        {!emailEnabled ? <Note>Email delivery is not configured. Share the invitation link instead.</Note> : null}
        {link ? (
          <View className="gap-2">
            <Input label={`Invitation link (${roleLabel(link.role)})`} value={link.url} editable={false} selectTextOnFocus />
            <View className="flex-row">
              <Button label="Share link" icon="send" variant="outline" onPress={() => void Share.share({ message: link.url })} />
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
      <LabelTip label="Channels" text="A guest sees only these private channels and the people in them." />
      {conversations.isPending ? <Note>Loading channels…</Note> : null}
      {conversations.isError && !conversations.data ? (
        <View className="flex-row items-center gap-2">
          <Note error>Channels could not be loaded.</Note>
          <Button label="Retry" variant="outline" size="sm" disabled={conversations.isFetching} onPress={() => void conversations.refetch()} />
        </View>
      ) : null}
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
  if (invitations.isPending) return <SettingsCard title="Pending invitations"><Note loading>Loading invitations…</Note></SettingsCard>
  if (invitations.isError) return <SettingsCard title="Pending invitations"><Note error>Invitations could not be loaded.</Note></SettingsCard>
  if (invitations.data.length === 0) return null
  return (
    <SettingsCard title="Pending invitations" flush>
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
