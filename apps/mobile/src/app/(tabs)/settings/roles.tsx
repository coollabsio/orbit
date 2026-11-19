import type { Permission, RoleProjectGrant } from '@orbit/api/generated/types.gen'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest, Dialog } from '../../../components/settings/Dialog'
import { Badge, Note, OptionText, RowIcon, Select, SettingsCard, SettingsRow, SettingsShell, Toggle, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { grantProjectName, type MemberProjectGrant, PERMISSION_LABELS, projectGrantLines, roleSummary, type WorkspaceRoleRecord } from '../../../data/account-api'
import { useMemberProjectGrants, useRevokeProjectGrant, useRoleWrites, useSetProjectGrant, useWorkspaceRoles } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { useMembers, useProjects } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { Button, IconButton, Input, LabelTip, Text } from '../../../ui'

/** The form open: a new role, or the role being edited. */
type Editing = { role: WorkspaceRoleRecord | null }

/** The project access form open: a new grant, or the grant being edited. */
type EditingGrant = { grant: MemberProjectGrant | null }

/** The web's Roles page: the custom roles with their member counts and permissions; create, edit and delete them. */
export default function RolesScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('roles.manage')
  const roles = useWorkspaceRoles(canManage)
  const projects = useProjects(canManage)
  const grants = useMemberProjectGrants(canManage)
  const { remove } = useRoleWrites()
  const revoke = useRevokeProjectGrant()
  const [editing, setEditing] = useState<Editing | null>(null)
  const [editingGrant, setEditingGrant] = useState<EditingGrant | null>(null)
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

  const askRevoke = (grant: MemberProjectGrant) =>
    setConfirm({
      title: `Revoke ${grant.display_name}'s access?`,
      description: `${grant.display_name} loses the permissions held directly in ${grant.project_name}. A role or workspace permission still applies.`,
      confirmLabel: 'Revoke',
      danger: true,
      onConfirm: () => revoke.mutate(grant),
    })

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => Promise.all([roles.refetch(), grants.refetch()])}>
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

      <SettingsCard title="Project access" description="Members who hold a permission in a single project directly, for example the creator of a project, who manages it until you revoke this."
        actions={<Button label="Add access" icon="plus" disabled={!roles.data || !grants.data} onPress={() => setEditingGrant({ grant: null })} />}
        flush
      >
        {grants.isPending ? <View className="p-4"><Note loading>Loading project access…</Note></View> : null}
        {grants.isError ? <View className="p-4"><Note error>Project access could not be loaded. {errorMessage(grants.error)}</Note></View> : null}
        {grants.data && grants.data.length === 0 ? <View className="p-4"><Note>No member holds a permission directly in a project.</Note></View> : null}
        {grants.data?.map((grant, index) => (
          <SettingsRow key={`${grant.project_id}:${grant.user_id}`} first={index === 0}>
            <RowIcon icon="folder" />
            <View className="min-w-0 flex-1 gap-0.5">
              <Text className="font-noto-medium text-[13px] text-foreground">{grant.display_name}</Text>
              <Text className="text-[12px] text-muted-foreground/70">{grant.project_name}</Text>
              <View className="mt-1 flex-row flex-wrap gap-1">
                {grant.permissions.map((permission) => (
                  <Badge key={permission} label={PERMISSION_LABELS[permission].label} tone="secondary" />
                ))}
              </View>
            </View>
            <View className="gap-1">
              <Button label="Edit" variant="ghost" size="sm" disabled={!roles.data} onPress={() => setEditingGrant({ grant })} />
              <Button label="Revoke" variant="ghost" size="sm" disabled={revoke.isPending} onPress={() => askRevoke(grant)} />
            </View>
          </SettingsRow>
        ))}
        {revoke.isError ? <View className="p-4"><Note error>The access could not be revoked. {errorMessage(revoke.error)}</Note></View> : null}
      </SettingsCard>

      {editing && roles.data ? <RoleDialog key={editing.role?.id ?? 'new'} role={editing.role} grantable={roles.data.grantable_permissions} projectPermissions={roles.data.project_permissions} projects={projects.data ?? []} onClose={() => setEditing(null)} /> : null}
      {editingGrant && roles.data && grants.data ? <GrantDialog key={editingGrant.grant ? `${editingGrant.grant.project_id}:${editingGrant.grant.user_id}` : 'new'} grant={editingGrant.grant} grants={grants.data} projectPermissions={roles.data.project_permissions} projects={projects.data ?? []} onClose={() => setEditingGrant(null)} /> : null}
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

/**
 * A member's direct permissions in one project: pick an ordinary member (base role Member) and a project for a new grant,
 * or edit a listed one with both fixed. Saving replaces what the member held there; a pair that already has a grant starts
 * from its permissions.
 */
function GrantDialog({ grant, grants, projectPermissions, projects, onClose }: { grant: MemberProjectGrant | null; grants: readonly MemberProjectGrant[]; projectPermissions: readonly Permission[]; projects: readonly Project[]; onClose: () => void }) {
  const members = useMembers()
  const write = useSetProjectGrant()
  const choosable = (members.data ?? []).filter((member) => member.role === 'member').sort((a, b) => a.display_name.localeCompare(b.display_name))
  const [userId, setUserId] = useState(grant?.user_id ?? '')
  const [projectId, setProjectId] = useState(grant?.project_id ?? projects[0]?.id ?? '')
  // null until a switch is touched: the switches then show what the chosen member already holds in the chosen project
  const [touched, setTouched] = useState<Permission[] | null>(null)
  const user = userId || choosable[0]?.user_id || ''
  const permissions = touched ?? grants.find((other) => other.user_id === user && other.project_id === projectId)?.permissions ?? []
  const ready = user !== '' && projectId !== '' && permissions.length > 0 && !write.isPending

  const pickUser = (next: string) => {
    setUserId(next)
    setTouched(null)
  }
  const pickProject = (next: string) => {
    setProjectId(next)
    setTouched(null)
  }

  const save = () => {
    if (!ready) return
    // keep the server's display order, whatever order the switches were turned on in
    write.mutate({ user_id: user, project_id: projectId, permissions: projectPermissions.filter((permission) => permissions.includes(permission)) }, { onSuccess: onClose })
  }

  return (
    <Dialog
      visible
      title={grant ? `Edit ${grant.display_name}'s access` : 'Add project access'}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label="Save" disabled={!ready} onPress={save} />
        </>
      }
    >
      {grant ? (
        <View className="gap-0.5">
          <Text className="font-noto-medium text-[13px] text-foreground">{grant.display_name}</Text>
          <Text className="text-[12px] text-muted-foreground/70">{grant.project_name}</Text>
        </View>
      ) : (
        <>
          <View className="gap-1.5">
            <LabelTip label="Member" text="Only members whose base role is Member. Owners and admins already hold every permission." />
            {members.isPending ? <Note loading>Loading members…</Note> : null}
            {members.isError ? <Note error>Members could not be loaded. {errorMessage(members.error)}</Note> : null}
            {members.data && choosable.length === 0 ? <Note>No member has the Member role.</Note> : null}
            {choosable.length > 0 ? <Select label="Member" value={user} options={choosable.map((member) => ({ value: member.user_id, label: member.display_name }))} onChange={pickUser} /> : null}
          </View>
          <View className="gap-1.5">
            <LabelTip label="Project" text="The project these permissions apply in." />
            {projects.length === 0 ? <Note>No project to choose.</Note> : <Select label="Project" value={projectId} options={projects.map((project) => ({ value: project.id, label: project.name }))} onChange={pickProject} />}
          </View>
        </>
      )}
      <View className="gap-3">
        <LabelTip label="Permissions" text="Saving replaces what the member holds directly in this project. A role or workspace permission still applies." />
        {projectPermissions.map((permission) => {
          const { label, help } = PERMISSION_LABELS[permission]
          return (
            <View key={permission} className="flex-row items-center gap-3">
              <OptionText title={label} help={help} />
              <Toggle label={label} value={permissions.includes(permission)} onChange={(next) => setTouched(next ? [...permissions, permission] : permissions.filter((other) => other !== permission))} />
            </View>
          )
        })}
      </View>
      {write.isError ? <Note error>{errorMessage(write.error)}</Note> : null}
    </Dialog>
  )
}
