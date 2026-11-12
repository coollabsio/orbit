import { toast } from 'sonner'
import { useState, type FormEvent } from 'react'
import { UserTag } from 'reicon-react'
import { ApiProblem } from '@orbit/api/problem'
import type { Permission, WorkspaceRoleRecord } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { LoadingText } from '@/components/ui/spinner'
import { useCreateWorkspaceRole, useDeleteWorkspaceRole, useUpdateWorkspaceRole, useWorkspaceRoles } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { RequiredMark, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'

type GrantablePermission = Exclude<Permission, 'workspace.delete' | 'workspace.transfer' | 'billing.manage' | 'roles.manage' | 'workspace.export_personal'>

const PERMISSION_LABELS: Record<GrantablePermission, string> = {
  'workspace.update': 'Change workspace settings',
  'members.manage': 'Invite and manage members',
  'audit.view': 'View the audit log',
  'api_tokens.manage': 'Manage API tokens',
  'integrations.manage': 'Manage integrations and webhooks',
  'teamspaces.delete': 'Delete teamspaces',
  'pages.purge': 'Permanently delete pages and tasks',
  'views.manage_shared': 'Manage shared views',
  'comments.moderate': 'Moderate comments',
  'chat.manage': 'Manage chat',
  'mail.manage': 'Manage mail',
  'storage.manage': 'Manage file storage',
  'workspace.export': 'Export the workspace',
}

const permissionLabel = (permission: Permission) => PERMISSION_LABELS[permission as GrantablePermission] ?? permission

const failure = (error: unknown, fallback: string) => error instanceof ApiProblem ? error.detail : fallback

const memberCount = (count: number) => count === 1 ? '1 member' : `${count} members`

export function RolesPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('roles.manage')
  const roles = useWorkspaceRoles(workspace.id, canManage)
  const deleteRole = useDeleteWorkspaceRole(workspace.id)
  const [editing, setEditing] = useState<WorkspaceRoleRecord | 'new'>()

  if (!canManage) return <SettingsCard title="Roles"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage roles.</p></SettingsCard>

  async function remove(role: WorkspaceRoleRecord) {
    const description = role.member_count > 0 ? `${memberCount(role.member_count)} with this role become ordinary members.` : undefined
    if (!await confirmAction({ title: `Delete ${role.name}?`, description, confirmLabel: 'Delete', danger: true })) return
    deleteRole.mutate(role.id, { onError: (error) => toast.error(failure(error, 'The role could not be deleted.')) })
  }

  return <>
    <SettingsCard
      title="Roles"
      description="A custom role gives ordinary members extra permissions on top of the member role. Give a member a role from their row in Members."
      actions={<Button type="button" disabled={!roles.data} onClick={() => setEditing('new')}>Add role</Button>}
      flush
    >
      <div className="flex flex-col divide-y">
        {roles.isPending ? <SettingsRow><LoadingText>Loading roles…</LoadingText></SettingsRow> : null}
        {roles.isError ? <SettingsRow role="alert">Roles could not be loaded. <Button variant="ghost" onClick={() => void roles.refetch()}>Retry</Button></SettingsRow> : null}
        {roles.data?.items.length === 0 ? <SettingsRow className="text-muted-foreground">No custom roles yet.</SettingsRow> : null}
        {roles.data?.items.map((role) => <SettingsRow key={role.id} data-role-row className="flex-nowrap">
          <RowIcon><UserTag /></RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">{role.name}<span className="text-xs font-normal text-muted-foreground">{memberCount(role.member_count)}</span></span>
            <span className="flex flex-wrap gap-1">
              {role.permissions.length > 0
                ? role.permissions.map((permission) => <Badge key={permission} variant="secondary">{permissionLabel(permission)}</Badge>)
                : <span className="text-xs text-muted-foreground">No extra permissions</span>}
            </span>
          </div>
          <span className="flex shrink-0 gap-1">
            <Button variant="ghost" type="button" onClick={() => setEditing(role)}>Edit</Button>
            <Button variant="ghost" type="button" disabled={deleteRole.isPending} onClick={() => void remove(role)}>Delete</Button>
          </span>
        </SettingsRow>)}
      </div>
    </SettingsCard>
    {editing && roles.data ? <RoleDialog
      workspaceId={workspace.id}
      role={editing === 'new' ? undefined : editing}
      grantable={roles.data.grantable_permissions}
      onClose={() => setEditing(undefined)}
    /> : null}
  </>
}

function RoleDialog({ workspaceId, role, grantable, onClose }: {
  workspaceId: string
  role?: WorkspaceRoleRecord
  grantable: Permission[]
  onClose: () => void
}) {
  const createRole = useCreateWorkspaceRole(workspaceId)
  const updateRole = useUpdateWorkspaceRole(workspaceId)
  const [name, setName] = useState(role?.name ?? '')
  const [permissions, setPermissions] = useState<Permission[]>(role?.permissions ?? [])
  const [error, setError] = useState<string>()
  const pending = createRole.isPending || updateRole.isPending

  async function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed) return
    // keep the server's display order, whatever order the boxes were ticked in
    const ordered = grantable.filter((permission) => permissions.includes(permission))
    setError(undefined)
    try {
      if (role) await updateRole.mutateAsync({ roleId: role.id, body: { expected_version: role.version, name: trimmed, permissions: ordered } })
      else await createRole.mutateAsync({ name: trimmed, permissions: ordered })
      onClose()
    } catch (caught) {
      setError(failure(caught, role ? 'The role could not be saved.' : 'The role could not be created.'))
    }
  }

  return <Modal title={role ? `Edit ${role.name}` : 'Add role'} onClose={onClose} dismissible={!pending} className="sm:max-w-lg">
    <form className="grid gap-4" onSubmit={(event) => void submit(event)}>
      <Field>
        <FieldLabel htmlFor="role-name">Name <RequiredMark /></FieldLabel>
        <Input id="role-name" value={name} required maxLength={40} onChange={(event) => setName(event.target.value)} placeholder="Moderator" />
      </Field>
      <div role="group" aria-labelledby="role-permissions" className="grid gap-3">
        <span className="flex items-center gap-1.5"><span id="role-permissions" className="text-sm font-medium">Permissions</span><InfoTip text="What a member with this role may do beyond an ordinary member." /></span>
        {grantable.map((permission) => <Label key={permission} className="flex items-center gap-2 font-normal">
          <Checkbox
            checked={permissions.includes(permission)}
            onCheckedChange={(checked) => setPermissions(checked ? [...permissions, permission] : permissions.filter((other) => other !== permission))}
          />
          {permissionLabel(permission)}
        </Label>)}
      </div>
      {error ? <p className="m-0 text-destructive" role="alert">{error}</p> : null}
      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" disabled={pending} />}>Cancel</DialogClose>
        <Button type="submit" disabled={pending || !name.trim()}>{role ? 'Save' : 'Add role'}</Button>
      </DialogFooter>
    </form>
  </Modal>
}
