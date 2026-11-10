import { Redirect, router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { Note, SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { guestSettings } from '../../../data/guest'
import { useAuthOptions, useMemberWrites, useRenameWorkspace } from '../../../hooks/account'
import { useIsGuest, useOrbit } from '../../../orbit-context'
import { useMembers } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { Button, Input, Text } from '../../../ui'

/**
 * The dock's Settings: the web's workspace General page (name, leave, version). The account pages and Sign out are in
 * the account menu, as on the web. A guest has no workspace settings, so it gets its account settings.
 */
export default function GeneralScreen() {
  const guest = useIsGuest()
  if (guest) return <Redirect href={guestSettings} />
  return <General />
}

function General() {
  const { workspace } = useOrbit()
  const version = useAuthOptions().data?.version

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
      {workspace.permissions.includes('workspace.update') ? <WorkspaceName key={workspace.id} /> : null}
      <LeaveWorkspace key={`leave-${workspace.id}`} />
      {version ? <Text className="text-center text-[12px] leading-[16px] text-muted-foreground">Orbit v{version}</Text> : null}
    </SettingsShell>
  )
}

function WorkspaceName() {
  const { workspace } = useOrbit()
  const rename = useRenameWorkspace()
  const [name, setName] = useState(workspace.name)
  const dirty = name.trim() !== workspace.name
  return (
    <SettingsCard title="Workspace">
      <Input label="Name" value={name} onChangeText={setName} maxLength={100} returnKeyType="done" onSubmitEditing={() => dirty && name.trim() && rename.mutate(name.trim())} />
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

/** The web's underlined link inside a sentence. */
function InlineLink({ label, href }: { label: string; href: '/settings/danger-zone' | '/settings/members' }) {
  return (
    <Text accessibilityRole="link" onPress={() => router.push(href)} className="text-[13px] leading-[19px] text-foreground underline">
      {label}
    </Text>
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
      <SettingsCard title="Leave workspace">
        {billing ? (
          <Note>
            The owner cannot leave. Ownership transfer is not available on Orbit Cloud yet. To remove the workspace, delete it in{' '}
            <InlineLink label="Danger zone" href="/settings/danger-zone" />.
          </Note>
        ) : (
          <Note>
            To leave, first transfer ownership to another member in <InlineLink label="Members" href="/settings/members" />, or delete the workspace in{' '}
            <InlineLink label="Danger zone" href="/settings/danger-zone" />.
          </Note>
        )}
      </SettingsCard>
    )
  }
  return (
    <SettingsCard title="Leave workspace">
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
