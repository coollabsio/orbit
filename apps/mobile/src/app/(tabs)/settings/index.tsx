import { router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { ACCOUNT_NAV, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { useAuthOptions, useMemberWrites, useRenameWorkspace } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { useTheme } from '../../../theme'
import { Button, Icon, Input } from '../../../ui'

/**
 * The dock's Settings: the web's workspace General page (name, leave), and the account pages,
 * the server and Sign out, which the web keeps in the account menu.
 */
export default function GeneralScreen() {
  const { workspace, session, user, signOut } = useOrbit()
  const { colors } = useTheme()
  const [signingOut, setSigningOut] = useState(false)
  const [signOutError, setSignOutError] = useState<string | null>(null)

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
      {workspace.permissions.includes('workspace.update') ? <WorkspaceName key={workspace.id} /> : null}

      <SettingsCard title="Account" description={`${user.email} on ${session.server}`} flush>
        {ACCOUNT_NAV.map((item, index) => (
          <SettingsRow key={item.href} first={index === 0} label={item.label} onPress={() => router.push(item.href)}>
            <RowIcon icon={item.icon} />
            <RowText title={item.label} />
            <Icon name="chevronRight" size={14} color={colors.mutedForeground} />
          </SettingsRow>
        ))}
        <View className="p-4 border-t border-border gap-2 items-start">
          <Button
            label="Sign out"
            variant="outline"
            icon="logout"
            loading={signingOut}
            onPress={() => {
              setSigningOut(true)
              setSignOutError(null)
              signOut().catch((error: unknown) => {
                setSigningOut(false)
                setSignOutError(errorMessage(error))
              })
            }}
          />
          {signOutError ? <Note error>{signOutError}</Note> : null}
        </View>
      </SettingsCard>

      <LeaveWorkspace key={`leave-${workspace.id}`} />
    </SettingsShell>
  )
}

function WorkspaceName() {
  const { workspace } = useOrbit()
  const rename = useRenameWorkspace()
  const [name, setName] = useState(workspace.name)
  const dirty = name.trim() !== workspace.name
  return (
    <SettingsCard title="Workspace" description="Rename this workspace.">
      <Input label="Name" required value={name} onChangeText={setName} maxLength={100} returnKeyType="done" onSubmitEditing={() => dirty && name.trim() && rename.mutate(name.trim())} />
      {rename.isError ? <View className="mt-3"><Note error>Workspace rename failed. {errorMessage(rename.error)}</Note></View> : null}
      {dirty ? (
        <View className="mt-4 flex-row gap-2 justify-end">
          <Button label="Discard" variant="outline" onPress={() => setName(workspace.name)} />
          <Button label="Save" loading={rename.isPending} disabled={!name.trim()} onPress={() => rename.mutate(name.trim())} />
        </View>
      ) : null}
    </SettingsCard>
  )
}

/** Any member may leave; the owner transfers ownership first (`can_remove` on your own row). */
function LeaveWorkspace() {
  const { workspace, user } = useOrbit()
  const members = useMembers()
  const { leave } = useMemberWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const membership = members.data?.find((member) => member.user_id === user.id)
  // Orbit Cloud refuses ownership transfer.
  const billing = useAuthOptions().data?.billing_enabled ?? false

  if (membership && !membership.can_remove) {
    return (
      <SettingsCard title="Leave workspace" description="You own this workspace.">
        <Note>
          {billing
            ? 'The owner cannot leave. Ownership transfer is not available on Orbit Cloud yet.'
            : 'To leave, first transfer ownership to another member in Members.'}
        </Note>
      </SettingsCard>
    )
  }
  return (
    <SettingsCard title="Leave workspace" description="Remove yourself from this workspace.">
      <View className="items-start gap-2">
        <Button
          label={leave.isPending ? 'Leaving…' : 'Leave workspace'}
          variant="destructive"
          disabled={!membership || leave.isPending}
          onPress={() =>
            setConfirm({
              title: `Leave ${workspace.name}?`,
              description: 'You lose access to its projects, tasks, docs and chat. To come back, someone must invite you again.',
              confirmLabel: 'Leave workspace',
              danger: true,
              onConfirm: () => membership && leave.mutate(membership),
            })
          }
        />
        {leave.isError ? <Note error>You could not leave the workspace. {errorMessage(leave.error)}</Note> : null}
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}
