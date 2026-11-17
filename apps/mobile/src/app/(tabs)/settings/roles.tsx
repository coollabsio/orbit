import type { Permission, RoleProjectGrant } from '@orbit/api/generated/types.gen'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest, Dialog } from '../../../components/settings/Dialog'
import { Badge, Note, OptionText, RowIcon, Select, SettingsCard, SettingsRow, SettingsShell, Toggle, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { grantProjectName, PERMISSION_LABELS, projectGrantLines, roleSummary, type WorkspaceRoleRecord } from '../../../data/account-api'
import { useRoleWrites, useWorkspaceRoles } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { useProjects } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { Button, IconButton, Input, LabelTip, Text } from '../../../ui'

/** The form open: a new role, or the role being edited. */
type Editing = { role: WorkspaceRoleRecord | null }

/** The web's Roles page: the custom roles with their member counts and permissions; create, edit and delete them. */
export default function RolesScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('roles.manage')
  const roles = useWorkspaceRoles(canManage)
  const projects = useProjects(canManage)
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
              {role.project_grants.length > 0 ? (
                <View className="mt-1 gap-0.5">
                  {projectGrantLines(role, projects.data ?? []).map((line, lineIndex) => (
                    <Text key={role.project_grants[lineIndex].project_id} className="text-[12px] text-muted-foreground">{line}</Text>
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

      {editing && roles.data ? <RoleDialog key={editing.role?.id ?? 'new'} role={editing.role} grantable={roles.data.grantable_permissions} projectPermissions={roles.data.project_permissions} projects={projects.data ?? []} onClose={() => setEditing(null)} /> : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsShell>
  )
}

/** Name and permission switches for a new role (`role: null`) or an edited one. The server checks the name. */
function RoleDialog({ role, grantable, projectPermissions, projects, onClose }: { role: WorkspaceRoleRecord | null; grantable: readonly Permission[]; projectPermissions: readonly Permission[]; projects: readonly Project[]; onClose: () => void }) {
  const { create, update } = useRoleWrites()
  const [name, setName] = useState(role?.name ?? '')
  const [permissions, setPermissions] = useState<Permission[]>(role?.permissions ?? [])
  const [grants, setGrants] = useState<RoleProjectGrant[]>(role?.project_grants ?? [])
  const write = role ? update : create
  const ready = name.trim() !== '' && !write.isPending

  const save = () => {
    if (!ready) return
    // keep the server's display order, whatever order the switches were turned on in
    const draft = {
      name,
      permissions: grantable.filter((permission) => permissions.includes(permission)),
      projectGrants: grants.map((grant) => ({ project_id: grant.project_id, permissions: projectPermissions.filter((permission) => grant.permissions.includes(permission)) })),
    }
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
        <LabelTip label="Workspace permissions" text="These apply across the workspace, in every project too." />
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
      {projectPermissions.length > 0 ? <ProjectGrants grants={grants} onChange={setGrants} projectPermissions={projectPermissions} projects={projects} /> : null}
      {write.isError ? <Note error>{errorMessage(write.error)}</Note> : null}
    </Dialog>
  )
}

type Project = { id: string; name: string }

/** One row per granted project: its picker, a switch per project-scoped permission and Remove. Each project is offered once. */
function ProjectGrants({ grants, onChange, projectPermissions, projects }: { grants: RoleProjectGrant[]; onChange: (grants: RoleProjectGrant[]) => void; projectPermissions: readonly Permission[]; projects: readonly Project[] }) {
  // a granted project that the list does not show (a private one) stays choosable under the grant's name
  const known = [...projects, ...grants.filter((grant) => !projects.some((project) => project.id === grant.project_id)).map((grant) => ({ id: grant.project_id, name: grantProjectName(grant, projects) }))]
  const free = (keep?: string) => known.filter((project) => project.id === keep || !grants.some((grant) => grant.project_id === project.id))
  const next = free()[0]
  const change = (index: number, grant: RoleProjectGrant) => onChange(grants.map((other, at) => (at === index ? grant : other)))

  return (
    <View className="gap-3">
      <LabelTip label="Project permissions" text="Give these permissions in single projects only. A permission switched on above already applies to every project. A project with nothing switched on is dropped." />
      {grants.map((grant, index) => (
        <View key={grant.project_id} className="gap-2 rounded-lg border border-border p-3">
          <View className="flex-row items-center gap-2">
            <View className="min-w-0 flex-1">
              <Select label="Project" value={grant.project_id} options={free(grant.project_id).map((project) => ({ value: project.id, label: project.name }))} onChange={(projectId) => change(index, { ...grant, project_id: projectId })} />
            </View>
            <IconButton icon="trash" label="Remove project" onPress={() => onChange(grants.filter((_, at) => at !== index))} />
          </View>
          {projectPermissions.map((permission) => {
            const { label, help } = PERMISSION_LABELS[permission]
            const on = grant.permissions.includes(permission)
            return (
              <View key={permission} className="flex-row items-center gap-3">
                <OptionText title={label} help={help} />
                <Toggle label={label} value={on} onChange={(value) => change(index, { ...grant, permissions: value ? [...grant.permissions, permission] : grant.permissions.filter((other) => other !== permission) })} />
              </View>
            )
          })}
        </View>
      ))}
      <View className="flex-row">
        <Button label="Add project" icon="plus" variant="outline" size="sm" disabled={!next} onPress={() => next && onChange([...grants, { project_id: next.id, permissions: [] }])} />
      </View>
    </View>
  )
}
