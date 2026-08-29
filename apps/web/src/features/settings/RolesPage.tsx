// Server settings › Roles: list with search + create, drag reorder, and an edit view with
// Display (name + color) and Manage Members tabs.
import { useState, type MouseEvent as ReactMouseEvent } from 'react'
import { useSearchParams } from 'react-router'
import { Add, ArrowLeft2, Edit, Magnifier, People, TickCircle, Trash, Xmark } from 'reicon-react'
import { assignMemberRoles, createRole, deleteRole, reorderRoles, updateRole } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import type { Role, User } from '../../mock/types'
import { ConfirmDeleteModal } from '../chat/components/ChannelModals'
import './server.css'

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
  return <span aria-hidden="true" className="fs-drop-line" data-position={position} />
}

export function RolesPage() {
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
      <div className="fs-role-edit">
        <div className="fs-role-rail">
          <div className="fs-role-rail-head">
            <button type="button" className="fs-role-back" onClick={() => setSelectedRoleId(null)}>
              <ArrowLeft2 size={16} />
              Back
            </button>
            <button type="button" className="fc-thread-icon-button" title="Create role" onClick={handleCreate}>
              <Add size={16} />
            </button>
          </div>
          <div className="fs-role-rail-list">
            {roles.map((role) => (
              <button
                key={role.id}
                type="button"
                className="fs-role-rail-item"
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
                <span className="fs-role-dot" style={{ backgroundColor: role.color }} />
                <span className="truncate">{role.name}</span>
              </button>
            ))}
          </div>
        </div>

        <div className="fs-role-panel">
          <div className="fs-role-panel-head">
            <h2>Edit Role — {selectedRole.name}</h2>
            <button type="button" className="fs-role-close" title="Close" onClick={() => setSelectedRoleId(null)}>
              <Xmark size={16} />
            </button>
          </div>

          <div className="fs-tabs">
            <button type="button" className="fs-tab" data-active={roleTab === 'display' ? 'true' : undefined} onClick={() => setRoleTab('display')}>
              Display
            </button>
            <button type="button" className="fs-tab" data-active={roleTab === 'members' ? 'true' : undefined} onClick={() => setRoleTab('members')}>
              Manage Members ({memberCount(members, selectedRole.id)})
            </button>
          </div>

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
    <div className="fs-page" data-wide>
      <div>
        <h2 className="fs-title">Roles</h2>
        <p className="fs-subtitle" data-strong>
          Use roles to group your server members.
        </p>
      </div>

      <div className="fs-row">
        <label className="fs-search">
          <Magnifier size={16} />
          <input type="text" className="fs-input" value={roleSearch} placeholder="Search Roles" onChange={(e) => setRoleSearch(e.target.value)} />
        </label>
        <button type="button" className="fs-btn" data-variant="primary" onClick={handleCreate}>
          Create Role
        </button>
      </div>

      <p className="fs-subtitle" data-strong>
        Members use the color of the highest role they have on this list.
      </p>

      <div>
        <div className="fs-roles-head">
          <span>Roles — {roles.length}</span>
          <span>Members</span>
          <span className="sr-only">Actions</span>
        </div>
        {filteredRoles.length === 0 ? (
          <p className="fs-roles-empty">No roles found.</p>
        ) : (
          <div className="fs-roles-list">
            {filteredRoles.map((role) => (
              <div
                key={role.id}
                className="fs-role-row"
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
                <button
                  type="button"
                  className="fs-role-name"
                  onClick={() => {
                    setSelectedRoleId(role.id)
                    setRoleTab('display')
                  }}
                >
                  <span className="fs-role-dot" style={{ backgroundColor: role.color }} />
                  <span className="truncate">{role.name}</span>
                </button>
                <div className="fs-role-count">
                  <span>{memberCount(members, role.id)}</span>
                  <People size={16} />
                </div>
                <div className="fs-role-actions">
                  <button
                    type="button"
                    className="fs-square-button"
                    title="Edit role"
                    onClick={() => {
                      setSelectedRoleId(role.id)
                      setRoleTab('display')
                    }}
                  >
                    <Edit size={16} />
                  </button>
                  <button type="button" className="fs-square-button" data-danger="true" title="Delete role" onClick={() => setDeleteRoleTarget(role)}>
                    <Trash size={16} />
                  </button>
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
      <div className="fs-section">
        <label className="fs-field-label" htmlFor="role-name">
          Role name <span className="fs-required">*</span>
        </label>
        <input
          id="role-name"
          type="text"
          className="fs-input"
          data-h10
          value={draftName}
          maxLength={50}
          onChange={(e) => setDraftName(e.target.value)}
          onBlur={saveIfChanged}
        />
      </div>

      <div>
        <h3 className="fs-field-label" style={{ marginBottom: 0 }}>
          Role color <span className="fs-required">*</span>
        </h3>
        <p className="fs-field-help">Members use the color of the highest role they have on the roles list.</p>
        <div className="fs-swatches">
          <label className="fs-swatch-custom" style={{ backgroundColor: draftColor }} title="Custom color">
            <input type="color" value={draftColor} aria-label={`${role.name} color`} onChange={(e) => setDraftColor(e.target.value)} onBlur={saveIfChanged} />
            <Edit size={16} />
          </label>
          {ROLE_COLORS.map((color) => (
            <button
              key={color}
              type="button"
              className="fs-swatch"
              style={{ backgroundColor: color }}
              title={color}
              onClick={() => {
                setDraftColor(color)
                if (color !== role.color) onUpdate({ color })
              }}
            >
              {draftColor.toLowerCase() === color.toLowerCase() ? <TickCircle size={16} /> : null}
            </button>
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
    <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
      <div className="fs-row">
        <label className="fs-search">
          <Magnifier size={16} />
          <input type="text" className="fs-input" value={memberSearch} placeholder="Search Members" onChange={(e) => onMemberSearch(e.target.value)} />
        </label>
        <button
          type="button"
          className="fs-btn"
          data-variant={addingMembers ? 'secondary' : 'primary'}
          onClick={() => {
            setAddingMembers((open) => !open)
            onMemberSearch('')
          }}
        >
          {addingMembers ? 'Done' : 'Add Members'}
        </button>
      </div>

      {addingMembers ? (
        <div className="fs-add-box">
          <div className="fs-add-box-title">Add Members</div>
          {unassignedMembers.length === 0 ? (
            <p className="fs-members-empty">{memberSearch.trim() ? 'No members found.' : 'All members already have this role.'}</p>
          ) : (
            <div className="fs-member-list">
              {unassignedMembers.map((member) => (
                <MemberRoleRow key={member.id} member={member} action="add" onClick={() => onToggleMember(member)} />
              ))}
            </div>
          )}
        </div>
      ) : null}

      <div className="fs-member-list">
        {assignedMembers.length === 0 ? (
          <p className="fs-members-empty" style={{ padding: '24px 0' }}>
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
    <div className="fs-member-row">
      <span className="fs-member-avatar" style={{ background: `color-mix(in srgb, ${member.color} 22%, transparent)`, color: member.color }}>
        {member.name.charAt(0).toUpperCase()}
      </span>
      <div className="fs-member-name">
        <strong>{member.name}</strong>
        <span>{member.handle}</span>
      </div>
      <button
        type="button"
        className="fs-member-action"
        data-action={action}
        title={action === 'remove' ? 'Remove from role' : 'Add to role'}
        aria-label={action === 'remove' ? `Remove ${member.name} from role` : `Add ${member.name} to role`}
        onClick={onClick}
      >
        {action === 'remove' ? <Xmark size={14} /> : <Add size={14} />}
      </button>
    </div>
  )
}
