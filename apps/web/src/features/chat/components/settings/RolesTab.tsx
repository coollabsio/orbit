// Server settings › Roles: list with search + create, drag reorder, and an edit view with
// Display (name + color) and Manage Members tabs.
import { useState, type MouseEvent as ReactMouseEvent } from 'react'
import { useSearchParams } from 'react-router'
import { ArrowLeft, Check, Edit as Pencil, Add as Plus, SearchNormal as Search, Trash as Trash2, People as Users, Xmark as X } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { assignMemberRoles, createRole, deleteRole, reorderRoles, updateRole } from '@/mock/actions'
import { useAppState } from '@/mock/store'
import type { Role, User } from '@/mock/types'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { cn } from 'cn'
import { InitialAvatar } from '../InitialAvatar'
import { RowButton } from '../RowButton'

const ROLE_COLORS = [
  '#99aab5', '#57f287', '#2ecc71', '#1abc9c', '#3498db', '#9b59b6', '#e91e63', '#f1c40f', '#e67e22', '#e74c3c', '#95a5a6',
  '#607d8b', '#11806a', '#1f8b4c', '#206694', '#71368a', '#ad1457', '#c27c0e', '#a84300', '#992d22', '#979c9f', '#546e7a',
]
const DEFAULT_ROLE_COLOR = '#99aab5'

type DropPosition = 'before' | 'after'
type RoleDropIndicator = { roleId: string; position: DropPosition }

function nextRoleName(roles: Role[]): string {
  const names = new Set(roles.map((role) => role.name))
  if (!names.has('New Role')) return 'New Role'
  let index = 2
  while (names.has(`New Role ${index}`)) index += 1
  return `New Role ${index}`
}

function memberCount(members: User[], roleId: string): number {
  return members.filter((member) => member.roleIds.includes(roleId)).length
}

function getDropPosition(element: HTMLElement, clientY: number): DropPosition {
  const rect = element.getBoundingClientRect()
  return clientY < rect.top + rect.height / 2 ? 'before' : 'after'
}

function getRoleDropTarget(clientX: number, clientY: number): RoleDropIndicator | null {
  const element = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-role-drop-id]')
  const roleId = element?.getAttribute('data-role-drop-id')
  if (!element || !roleId) return null
  return { roleId, position: getDropPosition(element, clientY) }
}

function moveRole(roles: Role[], draggedRoleId: string, targetRoleId: string, targetPosition: DropPosition): Role[] {
  const nextRoles = [...roles]
  const fromIndex = nextRoles.findIndex((role) => role.id === draggedRoleId)
  const targetIndex = nextRoles.findIndex((role) => role.id === targetRoleId)
  if (fromIndex === -1 || targetIndex === -1) return nextRoles
  const [draggedRole] = nextRoles.splice(fromIndex, 1)
  const adjustedTargetIndex = nextRoles.findIndex((role) => role.id === targetRoleId)
  if (adjustedTargetIndex === -1) return roles
  nextRoles.splice(targetPosition === 'after' ? adjustedTargetIndex + 1 : adjustedTargetIndex, 0, draggedRole)
  return nextRoles
}

function RoleDropLine({ position }: { position: DropPosition | null }) {
  if (!position) return null
  return (
    <span
      aria-hidden="true"
      className="pointer-events-none absolute inset-x-0 z-10 h-0.5 rounded-full bg-primary shadow-[0_0_0_1px_rgba(0,0,0,0.35)] data-[position=before]:top-0 data-[position=after]:bottom-0"
      data-position={position}
    />
  )
}

export function RolesTab() {
  const state = useAppState()
  const roles = [...state.roles].sort((a, b) => a.position - b.position)
  const members = state.users
  const [searchParams] = useSearchParams()
  // ?role=<id> opens a role directly in the edit view
  const [selectedRoleId, setSelectedRoleId] = useState<string | null>(() => searchParams.get('role'))
  const [roleSearch, setRoleSearch] = useState('')
  const [memberSearch, setMemberSearch] = useState('')
  const [roleTab, setRoleTab] = useState<'display' | 'members'>('display')
  const [dragRoleId, setDragRoleId] = useState<string | null>(null)
  const [dropIndicator, setDropIndicator] = useState<RoleDropIndicator | null>(null)
  const [deleteRoleTarget, setDeleteRoleTarget] = useState<Role | null>(null)

  function handleCreate() {
    const created = createRole(nextRoleName(roles), DEFAULT_ROLE_COLOR)
    setSelectedRoleId(created.id)
    setRoleTab('display')
  }

  function toggleMemberRole(member: User, roleId: string) {
    const current = new Set(member.roleIds)
    if (current.has(roleId)) current.delete(roleId)
    else current.add(roleId)
    assignMemberRoles(member.id, [...current])
  }

  function startRoleDrag(roleId: string) {
    setDragRoleId(roleId)
    setDropIndicator(null)
    function handleMouseUp(event: MouseEvent) {
      const target = getRoleDropTarget(event.clientX, event.clientY)
      if (target && target.roleId !== roleId) {
        reorderRoles(moveRole(roles, roleId, target.roleId, target.position).map((role) => role.id))
      }
      setDragRoleId(null)
      setDropIndicator(null)
    }
    document.addEventListener('mouseup', handleMouseUp, { once: true })
  }

  function handleRoleDragOver(roleId: string, element: HTMLElement, clientY: number) {
    if (!dragRoleId || dragRoleId === roleId) {
      setDropIndicator(null)
      return
    }
    const position = getDropPosition(element, clientY)
    setDropIndicator((current) => (current?.roleId === roleId && current.position === position ? current : { roleId, position }))
  }

  function handleRoleDragLeave(roleId: string) {
    setDropIndicator((current) => (current?.roleId === roleId ? null : current))
  }

  const selectedRole = roles.find((role) => role.id === selectedRoleId) ?? null
  const normalizedRoleSearch = roleSearch.trim().toLowerCase()
  const filteredRoles = normalizedRoleSearch ? roles.filter((role) => role.name.toLowerCase().includes(normalizedRoleSearch)) : roles
  const normalizedMemberSearch = memberSearch.trim().toLowerCase()
  const filteredMembers = normalizedMemberSearch
    ? members.filter((member) => `${member.name} ${member.handle}`.toLowerCase().includes(normalizedMemberSearch))
    : members

  if (selectedRole) {
    return (
      <div className="grid min-h-[calc(100vh-5rem)] grid-cols-[13rem_1fr] gap-8 max-[899px]:min-h-0 max-[899px]:grid-cols-[minmax(0,1fr)]">
        <div className="min-w-0 border-r border-border pr-4 max-[899px]:border-r-0 max-[899px]:pr-0">
          <div className="mb-6 flex items-center justify-between">
            <Button variant="ghost" className="-ml-2.5" onClick={() => setSelectedRoleId(null)}>
              <ArrowLeft className="size-4" />
              Back
            </Button>
            <Button variant="ghost" size="icon-sm" title="Create role" aria-label="Create role" onClick={handleCreate}>
              <Plus className="size-4" />
            </Button>
          </div>
          <div className="flex flex-col gap-1">
            {roles.map((role) => (
              <RowButton
                key={role.id}
                className="relative h-9 cursor-grab rounded-md px-3 text-sm leading-5 font-bold text-foreground hover:bg-sidebar-accent/60 data-[active=true]:bg-sidebar-accent data-[dragging=true]:opacity-50"
                data-role-drop-id={role.id}
                data-active={selectedRole.id === role.id ? 'true' : undefined}
                data-dragging={dragRoleId === role.id ? 'true' : undefined}
                onMouseMove={(event: ReactMouseEvent<HTMLButtonElement>) => handleRoleDragOver(role.id, event.currentTarget, event.clientY)}
                onMouseLeave={() => handleRoleDragLeave(role.id)}
                onMouseDown={(event) => {
                  if (event.button !== 0) return
                  startRoleDrag(role.id)
                }}
                onClick={() => {
                  setSelectedRoleId(role.id)
                  setRoleTab('display')
                }}
              >
                <RoleDropLine position={dropIndicator?.roleId === role.id ? dropIndicator.position : null} />
                <span className="size-3 shrink-0 rounded-full" style={{ backgroundColor: role.color }} />
                <span className="truncate">{role.name}</span>
              </RowButton>
            ))}
          </div>
        </div>

        <div className="min-w-0 max-w-xl">
          <div className="mb-8 flex items-start justify-between gap-4">
            <h2 className="min-w-0 truncate text-base leading-6 font-bold uppercase text-foreground">Edit Role — {selectedRole.name}</h2>
            <Button
              variant="ghost"
              size="icon"
              title="Close"
              aria-label="Close role editor"
              onClick={() => setSelectedRoleId(null)}
            >
              <X className="size-4" />
            </Button>
          </div>

          <Tabs className="mb-6 gap-0" value={roleTab} onValueChange={(value) => setRoleTab(value as typeof roleTab)}>
            <TabsList variant="line" className="w-full justify-start border-b border-border">
              <TabsTrigger value="display" className="flex-none">
                Display
              </TabsTrigger>
              <TabsTrigger value="members" className="flex-none">
                Manage Members ({memberCount(members, selectedRole.id)})
              </TabsTrigger>
            </TabsList>
          </Tabs>

          {roleTab === 'display' ? (
            <RoleDisplayPanel key={selectedRole.id} role={selectedRole} onUpdate={(updates) => updateRole(selectedRole.id, updates)} />
          ) : (
            <RoleMembersPanel
              role={selectedRole}
              members={filteredMembers}
              memberSearch={memberSearch}
              onMemberSearch={setMemberSearch}
              onToggleMember={(member) => toggleMemberRole(member, selectedRole.id)}
            />
          )}
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h2 className="text-xl leading-7 font-semibold text-foreground">Roles</h2>
        <p className="mt-0.5 text-sm leading-5 text-foreground">Use roles to group your server members.</p>
      </div>

      <div className="flex items-center gap-4">
        <InputGroup className="h-9 flex-1">
          <InputGroupAddon>
            <Search />
          </InputGroupAddon>
          <InputGroupInput type="text" value={roleSearch} placeholder="Search Roles" aria-label="Search roles" onChange={(e) => setRoleSearch(e.target.value)} />
        </InputGroup>
        <Button size="lg" onClick={handleCreate}>
          Create Role
        </Button>
      </div>

      <p className="mt-0.5 text-sm leading-5 text-foreground">Members use the color of the highest role they have on this list.</p>

      <div>
        <div className="grid grid-cols-[1fr_8rem_6rem] border-b border-border pb-2 text-xs leading-5 font-bold uppercase text-muted-foreground">
          <span>Roles — {roles.length}</span>
          <span className="-ml-3">Members</span>
          <span className="sr-only">Actions</span>
        </div>
        {filteredRoles.length === 0 ? (
          <p className="py-8 text-sm text-muted-foreground">No roles found.</p>
        ) : (
          <div className="[&>*+*]:border-t [&>*+*]:border-border">
            {filteredRoles.map((role) => (
              <div
                key={role.id}
                className="relative grid min-h-16 cursor-grab grid-cols-[1fr_8rem_6rem] items-center gap-3 py-2 select-none active:cursor-grabbing data-[dragging=true]:opacity-50"
                data-role-drop-id={role.id}
                data-dragging={dragRoleId === role.id ? 'true' : undefined}
                onMouseMove={(event: ReactMouseEvent<HTMLDivElement>) => handleRoleDragOver(role.id, event.currentTarget, event.clientY)}
                onMouseLeave={() => handleRoleDragLeave(role.id)}
                onMouseDown={(event) => {
                  if (event.button !== 0) return
                  startRoleDrag(role.id)
                }}
              >
                <RoleDropLine position={dropIndicator?.roleId === role.id ? dropIndicator.position : null} />
                <RowButton
                  className="gap-3 py-2 pr-3 text-sm leading-5 font-bold text-foreground hover:text-primary"
                  onClick={() => {
                    setSelectedRoleId(role.id)
                    setRoleTab('display')
                  }}
                >
                  <span className="size-3.5 shrink-0 rounded-full" style={{ backgroundColor: role.color }} />
                  <span className="truncate">{role.name}</span>
                </RowButton>
                <div className="flex items-center gap-1.5 text-sm leading-5 font-medium text-muted-foreground">
                  <span>{memberCount(members, role.id)}</span>
                  <Users className="size-4" />
                </div>
                <div className="flex items-center justify-end gap-2">
                  <Button
                    variant="outline"
                    size="icon"
                    title="Edit role"
                    aria-label={`Edit ${role.name}`}
                    onClick={() => {
                      setSelectedRoleId(role.id)
                      setRoleTab('display')
                    }}
                  >
                    <Pencil className="size-4" />
                  </Button>
                  <Button
                    variant="outline"
                    size="icon"
                    className="hover:bg-destructive/10 hover:text-destructive dark:hover:bg-destructive/10"
                    title="Delete role"
                    aria-label={`Delete ${role.name}`}
                    onClick={() => setDeleteRoleTarget(role)}
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {deleteRoleTarget ? (
        <ConfirmDeleteModal
          title="Delete role?"
          description={`This will permanently delete ${deleteRoleTarget.name} and remove it from assigned members.`}
          onClose={() => setDeleteRoleTarget(null)}
          onConfirm={() => {
            deleteRole(deleteRoleTarget.id)
            if (selectedRoleId === deleteRoleTarget.id) setSelectedRoleId(null)
            setDeleteRoleTarget(null)
          }}
        />
      ) : null}
    </div>
  )
}

function RoleDisplayPanel({ role, onUpdate }: { role: Role; onUpdate: (updates: { name?: string; color?: string }) => void }) {
  const [draftName, setDraftName] = useState(role.name)
  const [draftColor, setDraftColor] = useState(role.color)

  function saveIfChanged() {
    const updates: { name?: string; color?: string } = {}
    if (draftName.trim() && draftName.trim() !== role.name) updates.name = draftName.trim()
    if (draftColor !== role.color) updates.color = draftColor
    if (updates.name || updates.color) onUpdate(updates)
  }

  return (
    <div>
      <Field className="mb-8 border-b border-border pb-6">
        <FieldLabel htmlFor="role-name">
          Role name <span className="text-primary">*</span>
        </FieldLabel>
        <Input
          id="role-name"
          type="text"
          value={draftName}
          maxLength={50}
          onChange={(e) => setDraftName(e.target.value)}
          onBlur={saveIfChanged}
        />
      </Field>

      <div>
        <h3 className="text-sm leading-5 font-bold text-foreground">
          Role color <span className="text-primary">*</span>
        </h3>
        <p className="mb-3 text-sm leading-5 text-muted-foreground">Members use the color of the highest role they have on the roles list.</p>
        <div className="flex flex-wrap items-center gap-2">
          <label className="relative flex h-12 w-16 cursor-pointer items-center justify-center rounded-md border border-border text-white" style={{ backgroundColor: draftColor }} title="Custom color">
            {/* native color picker under the swatch; the Input primitive has no color variant */}
            <input type="color" className="absolute inset-0 cursor-pointer opacity-0" value={draftColor} aria-label={`${role.name} color`} onChange={(e) => setDraftColor(e.target.value)} onBlur={saveIfChanged} />
            <Pencil className="size-4 [filter:drop-shadow(0_1px_1px_rgba(0,0,0,0.4))]" />
          </label>
          {ROLE_COLORS.map((color) => (
            <Button
              key={color}
              size="icon-xs"
              className="rounded-md text-white"
              style={{ backgroundColor: color }}
              title={color}
              aria-label={`Use color ${color}`}
              onClick={() => {
                setDraftColor(color)
                if (color !== role.color) onUpdate({ color })
              }}
            >
              {draftColor.toLowerCase() === color.toLowerCase() ? <Check className="size-4 [filter:drop-shadow(0_1px_1px_rgba(0,0,0,0.4))]" /> : null}
            </Button>
          ))}
        </div>
      </div>
    </div>
  )
}

function RoleMembersPanel({
  role,
  members,
  memberSearch,
  onMemberSearch,
  onToggleMember,
}: {
  role: Role
  members: User[]
  memberSearch: string
  onMemberSearch: (value: string) => void
  onToggleMember: (member: User) => void
}) {
  const [addingMembers, setAddingMembers] = useState(false)
  const assignedMembers = members.filter((member) => member.roleIds.includes(role.id))
  const unassignedMembers = members.filter((member) => !member.roleIds.includes(role.id))

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center gap-4">
        <InputGroup className="h-9 flex-1">
          <InputGroupAddon>
            <Search />
          </InputGroupAddon>
          <InputGroupInput type="text" value={memberSearch} placeholder="Search Members" aria-label="Search members" onChange={(e) => onMemberSearch(e.target.value)} />
        </InputGroup>
        <Button
          size="lg"
          variant={addingMembers ? 'secondary' : 'default'}
          onClick={() => {
            setAddingMembers((open) => !open)
            onMemberSearch('')
          }}
        >
          {addingMembers ? 'Done' : 'Add Members'}
        </Button>
      </div>

      {addingMembers ? (
        <div className="rounded-lg border border-border bg-background p-2">
          <div className="px-2 pb-2 text-xs leading-5 font-bold uppercase text-muted-foreground">Add Members</div>
          {unassignedMembers.length === 0 ? (
            <p className="px-2 py-6 text-sm text-muted-foreground">{memberSearch.trim() ? 'No members found.' : 'All members already have this role.'}</p>
          ) : (
            <div className="flex flex-col gap-1">
              {unassignedMembers.map((member) => (
                <MemberRoleRow key={member.id} member={member} action="add" onClick={() => onToggleMember(member)} />
              ))}
            </div>
          )}
        </div>
      ) : null}

      <div className="flex flex-col gap-1">
        {assignedMembers.length === 0 ? (
          <p className="px-0 py-6 text-sm text-muted-foreground">
            {memberSearch.trim() ? 'No assigned members found.' : 'No members have this role.'}
          </p>
        ) : (
          assignedMembers.map((member) => <MemberRoleRow key={member.id} member={member} action="remove" onClick={() => onToggleMember(member)} />)
        )}
      </div>
    </div>
  )
}

function MemberRoleRow({ member, action, onClick }: { member: User; action: 'add' | 'remove'; onClick: () => void }) {
  return (
    <div className="flex min-h-10 items-center gap-3 rounded-lg px-2 py-1.5 transition-colors hover:bg-sidebar-accent/60">
      <InitialAvatar name={member.name} color={member.color} className="size-7 text-xs font-bold" />
      <div className="min-w-0 flex-1 text-sm leading-5">
        <strong className="font-bold text-foreground">{member.name}</strong>
        <span className="ml-1 text-muted-foreground">{member.handle}</span>
      </div>
      <Button
        variant={action === 'remove' ? 'secondary' : 'outline'}
        size="icon-xs"
        className={cn(
          'rounded-full text-muted-foreground hover:text-white',
          action === 'remove' ? 'hover:bg-destructive' : 'hover:bg-primary dark:hover:bg-primary',
        )}
        data-action={action}
        title={action === 'remove' ? 'Remove from role' : 'Add to role'}
        aria-label={action === 'remove' ? `Remove ${member.name} from role` : `Add ${member.name} to role`}
        onClick={onClick}
      >
        {action === 'remove' ? <X className="size-3.5" /> : <Plus className="size-3.5" />}
      </Button>
    </div>
  )
}
