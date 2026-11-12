import type { Permission } from '@orbit/api/generated/types.gen'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest, Dialog } from '../../../components/settings/Dialog'
import { Badge, FieldLabel, Note, OptionText, RowIcon, SettingsCard, SettingsRow, SettingsShell, Toggle, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { PERMISSION_LABELS, roleSummary, type WorkspaceRoleRecord } from '../../../data/account-api'
import { useRoleWrites, useWorkspaceRoles } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { Button, Input, Text } from '../../../ui'

/** The form open: a new role, or the role being edited. */
type Editing = { role: WorkspaceRoleRecord | null }

/** The web's Roles page: the custom roles with their member counts and permissions; create, edit and delete them. */
export default function RolesScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('roles.manage')
  const roles = useWorkspaceRoles(canManage)
  const { remove } = useRoleWrites()
  const [editing, setEditing] = useState<Editing | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)

  if (!canManage) {
    return (
      <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
        <SettingsCard title="Roles">
          <Note>Only workspace owners and admins can manage roles.</Note>
        </SettingsCard>
      </SettingsShell>
    )
  }

  const askDelete = (role: WorkspaceRoleRecord) =>
    setConfirm({
      title: `Delete ${role.name}?`,
      description: role.member_count === 0 ? 'No member has this role.' : `Its ${role.member_count === 1 ? 'member becomes an ordinary member' : `${role.member_count} members become ordinary members`}.`,
      confirmLabel: 'Delete role',
      danger: true,
      onConfirm: () => remove.mutate(role.id),
    })

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => roles.refetch()}>
      <SettingsCard
        title="Custom roles"
        description="A custom role gives ordinary members extra permissions on top of the Member role. Owners and admins already hold every permission."
        actions={<Button label="New role" icon="plus" disabled={!roles.data} onPress={() => setEditing({ role: null })} />}
        flush
      >
        {roles.isPending ? <View className="p-4"><Note loading>Loading roles…</Note></View> : null}
        {roles.isError ? <View className="p-4"><Note error>Roles could not be loaded.</Note></View> : null}
        {roles.data && roles.data.items.length === 0 ? <View className="p-4"><Note>No custom roles yet. Create one, then give it to members on the Members page.</Note></View> : null}
        {roles.data?.items.map((role, index) => (
          <SettingsRow key={role.id} first={index === 0}>
            <RowIcon icon="shieldUser" />
            <View className="min-w-0 flex-1 gap-0.5">
              <Text className="font-noto-medium text-[13px] text-foreground">{role.name}</Text>
              <Text className="text-[12px] text-muted-foreground/70">{roleSummary(role)}</Text>
              {role.permissions.length > 0 ? (
                <View className="mt-1 flex-row flex-wrap gap-1">
                  {role.permissions.map((permission) => (
                    <Badge key={permission} label={PERMISSION_LABELS[permission].label} tone="secondary" />
                  ))}
                </View>
              ) : null}
            </View>
            <View className="gap-1">
              <Button label="Edit" variant="ghost" size="sm" onPress={() => setEditing({ role })} />
              <Button label="Delete" variant="ghost" size="sm" disabled={remove.isPending} onPress={() => askDelete(role)} />
            </View>
          </SettingsRow>
        ))}
        {remove.isError ? <View className="p-4"><Note error>The role could not be deleted. {errorMessage(remove.error)}</Note></View> : null}
      </SettingsCard>

      {editing && roles.data ? <RoleDialog key={editing.role?.id ?? 'new'} role={editing.role} grantable={roles.data.grantable_permissions} onClose={() => setEditing(null)} /> : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsShell>
  )
}

/** Name and permission switches for a new role (`role: null`) or an edited one. The server checks the name. */
function RoleDialog({ role, grantable, onClose }: { role: WorkspaceRoleRecord | null; grantable: readonly Permission[]; onClose: () => void }) {
  const { create, update } = useRoleWrites()
  const [name, setName] = useState(role?.name ?? '')
  const [permissions, setPermissions] = useState<Permission[]>(role?.permissions ?? [])
  const write = role ? update : create
  const ready = name.trim() !== '' && !write.isPending

  const save = () => {
    if (!ready) return
    // keep the server's display order, whatever order the switches were turned on in
    const draft = { name, permissions: grantable.filter((permission) => permissions.includes(permission)) }
    if (role) update.mutate({ role, draft }, { onSuccess: onClose })
    else create.mutate(draft, { onSuccess: onClose })
  }

  return (
    <Dialog
      visible
      title={role ? `Edit ${role.name}` : 'New role'}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={role ? 'Save changes' : 'Create role'} disabled={!ready} onPress={save} />
        </>
      }
    >
      <Input label="Name" required value={name} maxLength={40} onChangeText={setName} placeholder="Moderator" autoCorrect={false} help="Up to 40 characters. Owner, Admin, Member and Guest are taken." />
      <View className="gap-3">
        <FieldLabel>Permissions</FieldLabel>
        {grantable.map((permission) => {
          const { label, help } = PERMISSION_LABELS[permission]
          const on = permissions.includes(permission)
          return (
            <View key={permission} className="flex-row items-center gap-3">
              <OptionText title={label} help={help} />
              <Toggle label={label} value={on} onChange={(next) => setPermissions(next ? [...permissions, permission] : permissions.filter((other) => other !== permission))} />
            </View>
          )
        })}
      </View>
      {write.isError ? <Note error>{errorMessage(write.error)}</Note> : null}
    </Dialog>
  )
}
