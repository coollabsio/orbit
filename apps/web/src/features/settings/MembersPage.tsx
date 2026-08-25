import { useMemo, useState } from 'react'
import { Add, ArrowDown2, ArrowRight, Copy, Notification, People, SearchNormal, X } from 'reicon-react'
import { Dropdown } from '../../components/ui/Dropdown'
import { EmptyState } from '../../components/ui/EmptyState'
import { Listbox } from '../../components/ui/Listbox'
import { removeUser, setUserRole } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import type { User } from '../../mock/types'
import { SettingsCard } from './SettingsCard'

type Role = User['role']
type Sort = 'name_asc' | 'name_desc' | 'email_asc' | 'role'

const ROLES: Role[] = ['Owner', 'Admin', 'Member', 'Visitor']
const PAGE_SIZES = [10, 25, 50, 100]

function initial(user: User) {
  return (user.name || user.email).charAt(0).toUpperCase()
}

export function MembersPage() {
  const state = useAppState()
  const me = state.users.find((u) => u.id === state.currentUserId)
  const canManage = me?.role === 'Owner' || me?.role === 'Admin'

  const [search, setSearch] = useState('')
  const [roleFilter, setRoleFilter] = useState<'all' | Role>('all')
  const [sortBy, setSortBy] = useState<Sort>('name_asc')
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(10)

  const [inviteEmail, setInviteEmail] = useState('')
  const [inviteRole, setInviteRole] = useState<Role>('Member')
  const [inviteLink, setInviteLink] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase()
    const list = state.users.filter((u) => {
      const matchesSearch =
        !query || [u.name, u.email, u.role].some((v) => v.toLowerCase().includes(query))
      const matchesRole = roleFilter === 'all' || u.role === roleFilter
      return matchesSearch && matchesRole
    })
    return list.sort((a, b) => {
      if (sortBy === 'name_desc') return b.name.localeCompare(a.name)
      if (sortBy === 'email_asc') return a.email.localeCompare(b.email)
      if (sortBy === 'role') return a.role.localeCompare(b.role)
      return a.name.localeCompare(b.name)
    })
  }, [state.users, search, roleFilter, sortBy])

  const lastPage = Math.max(1, Math.ceil(filtered.length / perPage))
  const currentPage = Math.min(page, lastPage)
  const firstRow = filtered.length === 0 ? 0 : (currentPage - 1) * perPage + 1
  const lastRow = Math.min(currentPage * perPage, filtered.length)
  const visible = filtered.slice((currentPage - 1) * perPage, currentPage * perPage)

  const generateLink = (e: React.FormEvent) => {
    e.preventDefault()
    const email = inviteEmail.trim()
    if (!email) return
    const token = Math.random().toString(36).slice(2, 10)
    setInviteLink(`${window.location.origin}/invitations/${token}?email=${encodeURIComponent(email)}`)
    setCopied(false)
  }

  const copyLink = async () => {
    if (!inviteLink) return
    await navigator.clipboard.writeText(inviteLink)
    setCopied(true)
  }

  return (
    <>
      <SettingsCard title="Members" flush>
        <div className="members-toolbar">
          <div className="members-search">
            <SearchNormal size={14} />
            <input
              type="search"
              className="input"
              placeholder="Search members"
              aria-label="Search members"
              value={search}
              onChange={(e) => {
                setSearch(e.target.value)
                setPage(1)
              }}
            />
            {search ? (
              <button
                type="button"
                className="members-search-clear"
                aria-label="Clear search"
                onClick={() => {
                  setSearch('')
                  setPage(1)
                }}
              >
                <X size={12} />
              </button>
            ) : null}
          </div>
          <div className="members-filters">
            <Listbox<'all' | Role>
              className="members-filter-role"
              aria-label="Filter by role"
              value={roleFilter}
              options={[{ value: 'all', label: 'All roles' }, ...ROLES.map((role) => ({ value: role, label: role }))]}
              onChange={(value) => {
                setRoleFilter(value)
                setPage(1)
              }}
            />
            <Listbox<Sort>
              className="members-filter-sort"
              aria-label="Sort members"
              value={sortBy}
              options={[
                { value: 'name_asc', label: 'Name A–Z' },
                { value: 'name_desc', label: 'Name Z–A' },
                { value: 'email_asc', label: 'Email A–Z' },
                { value: 'role', label: 'Role' },
              ]}
              onChange={(value) => {
                setSortBy(value)
                setPage(1)
              }}
            />
          </div>
        </div>

        {filtered.length > 0 ? (
          <>
            <div className="data-table">
              <div className="data-table-header members-table-grid">
                <span>Name</span>
                <span>Email</span>
                <span>Role</span>
                <span style={{ textAlign: 'right' }}>Actions</span>
              </div>
              {visible.map((user) => (
                <div key={user.id} className="data-table-row members-table-grid">
                  <div className="members-name">
                    <span className="avatar-tile">{initial(user)}</span>
                    <span className="truncate members-name-text">{user.name}</span>
                    {user.id === state.currentUserId ? (
                      <span className="badge" data-tone="accent">
                        You
                      </span>
                    ) : null}
                  </div>
                  <div className="truncate data-table-cell-muted">{user.email}</div>
                  <div>
                    <span className="badge">{user.role}</span>
                  </div>
                  <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
                    {canManage && user.id !== state.currentUserId ? (
                      <Dropdown
                        align="right"
                        trigger={(open) => (
                          <button
                            type="button"
                            className="button members-manage"
                            aria-haspopup="menu"
                            aria-expanded={open}
                          >
                            Manage
                          </button>
                        )}
                      >
                        {(close) => (
                          <>
                            {ROLES.filter((role) => role !== user.role).map((role) => (
                              <button
                                key={role}
                                type="button"
                                className="popover-option"
                                onClick={() => {
                                  setUserRole(user.id, role)
                                  close()
                                }}
                              >
                                Make {role.toLowerCase()}
                              </button>
                            ))}
                            <div className="popover-separator" />
                            <button
                              type="button"
                              className="popover-option"
                              data-tone="danger"
                              onClick={() => {
                                removeUser(user.id)
                                close()
                              }}
                            >
                              Remove member
                            </button>
                          </>
                        )}
                      </Dropdown>
                    ) : null}
                  </div>
                </div>
              ))}
            </div>
            <footer className="pagination">
              <div className="pagination-summary">
                <span>
                  {firstRow}-{lastRow} of {filtered.length}
                </span>
                <Dropdown
                  trigger={() => (
                    <button type="button" className="pagination-size" aria-label="Items per page">
                      <span>{perPage}</span>
                      <ArrowDown2 size={12} />
                    </button>
                  )}
                >
                  {(close) => (
                    <>
                      {PAGE_SIZES.map((size) => (
                        <button
                          key={size}
                          type="button"
                          className="popover-option"
                          data-selected={size === perPage || undefined}
                          onClick={() => {
                            setPerPage(size)
                            setPage(1)
                            close()
                          }}
                        >
                          {size}
                        </button>
                      ))}
                    </>
                  )}
                </Dropdown>
              </div>
              <div className="pagination-nav">
                <button
                  type="button"
                  className="pagination-button"
                  aria-label="Previous page"
                  disabled={currentPage <= 1}
                  onClick={() => setPage(currentPage - 1)}
                >
                  <ArrowRight size={14} style={{ transform: 'rotate(180deg)' }} />
                </button>
                <button
                  type="button"
                  className="pagination-button"
                  aria-label="Next page"
                  disabled={currentPage >= lastPage}
                  onClick={() => setPage(currentPage + 1)}
                >
                  <ArrowRight size={14} />
                </button>
              </div>
            </footer>
          </>
        ) : (
          <div className="members-empty">
            <EmptyState
              size="sm"
              icon={People}
              title="No matching members"
              description="Try a different name, email address, or role."
            />
          </div>
        )}
      </SettingsCard>

      {canManage ? (
        <form onSubmit={generateLink}>
          <SettingsCard
            title="Invite a member"
            description="Create a reusable invitation link or deliver it by email."
            actions={
              <>
                <button type="button" className="button" disabled title="Email delivery is not configured">
                  <Notification size={14} />
                  Send email
                </button>
                <button type="submit" className="button button-primary">
                  <Add size={14} />
                  Generate link
                </button>
              </>
            }
          >
            <div className="settings-grid">
              <div className="settings-field">
                <label className="field-label" htmlFor="invite-email">
                  Email address <span className="field-required">*</span>
                </label>
                <input
                  id="invite-email"
                  type="email"
                  className="input"
                  placeholder="teammate@example.com"
                  required
                  value={inviteEmail}
                  onChange={(e) => setInviteEmail(e.target.value)}
                />
              </div>
              <div className="settings-field">
                <label className="field-label" htmlFor="invite-role">
                  Role
                </label>
                <Listbox<Role>
                  id="invite-role"
                  value={inviteRole}
                  options={ROLES.filter((role) => role !== 'Owner' || me?.role === 'Owner').map((role) => ({
                    value: role,
                    label: role,
                  }))}
                  onChange={setInviteRole}
                />
              </div>
            </div>
            {inviteLink ? (
              <div className="invite-link-row">
                <div className="settings-field">
                  <label className="field-label" htmlFor="invite-link">
                    Invitation link ({inviteRole})
                  </label>
                  <input id="invite-link" className="input" value={inviteLink} readOnly />
                </div>
                <button type="button" className="button" onClick={copyLink}>
                  <Copy size={14} />
                  {copied ? 'Copied' : 'Copy'}
                </button>
              </div>
            ) : null}
          </SettingsCard>
        </form>
      ) : null}
    </>
  )
}
