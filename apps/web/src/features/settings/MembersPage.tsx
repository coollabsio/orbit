import { useMemo, useState } from 'react'
import { confirmAction } from '../../components/ui/confirmAction'
import { ArrowRight, Bell, Copy, Plus, Search, Users, X } from 'lucide-react'
import { cn } from 'cn'
import { Dropdown } from '../../components/ui/Dropdown'
import { EmptyState } from '../../components/ui/EmptyState'
import { Listbox } from '../../components/ui/Listbox'
import { Button, buttonVariants } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCurrentUser } from '../auth/api'
import { useChangeMemberRole, useCreateInvitation, useInvitations, useMembers, useRemoveMember, useRevokeInvitation, useTransferOwnership } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import type { User } from '../tasks/api/models'
import { INVITABLE_ROLES, canManageMember, canTransferOwnership } from './memberPermissions'
import { SettingsCard } from './SettingsCard'

type Role = User['role']
type Sort = 'name_asc' | 'name_desc' | 'email_asc' | 'role'

const ROLES: Role[] = ['Owner', 'Admin', 'Member']
const PAGE_SIZES = [10, 25, 50, 100]
const EMPTY_USERS: User[] = []

const FIELD_LABEL = 'mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground'
const GRID = 'grid grid-cols-1 gap-4 min-[900px]:grid-cols-2'
const FIELD = 'w-full min-w-0'
const REQ = 'inline-block font-semibold text-primary'
const TABLE = 'flex min-w-0 max-w-full flex-col overflow-x-auto [overscroll-behavior-x:contain]'
const TABLE_GRID = '[grid-template-columns:minmax(0,1.15fr)_minmax(0,1.55fr)_7rem_minmax(10rem,0.9fr)] max-[899px]:[grid-template-columns:minmax(0,1fr)_auto_auto] max-[899px]:[&>:nth-child(2)]:hidden'
const TABLE_HEADER = cn('grid h-10 items-center gap-4 border-b border-muted bg-black/[0.02] px-4 text-[13px] font-medium text-muted-foreground dark:bg-white/[0.02]', TABLE_GRID)
const TABLE_ROW = cn('grid min-h-12 items-center gap-4 border-b border-border px-4 py-2.5 transition-colors last:border-b-0 hover:bg-foreground/[0.02]', TABLE_GRID)
const BADGE = 'inline-flex items-center gap-1 rounded-full bg-sidebar-accent px-2 py-0.5 text-[10px] leading-[14px] font-medium whitespace-nowrap text-muted-foreground'
const POPOVER_OPTION = 'flex min-h-8 w-full cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm whitespace-nowrap outline-none hover:bg-accent hover:text-accent-foreground'
const AVATAR_TILE = 'inline-flex size-7 shrink-0 items-center justify-center rounded-lg bg-sidebar-accent text-[11px] font-semibold text-muted-foreground uppercase'

function initial(user: User) {
  return (user.name || user.email).charAt(0).toUpperCase()
}

export function MembersPage() {
  const { workspace } = useWorkspace()
  const canManage = workspace.role === 'owner' || workspace.role === 'admin'
  const currentUser = useCurrentUser()
  const membersQuery = useMembers(workspace.id)
  const createInvitation = useCreateInvitation(workspace.id)
  const invitations = useInvitations(canManage ? workspace.id : null)
  const revokeInvitation = useRevokeInvitation(workspace.id)
  const changeRole = useChangeMemberRole(workspace.id)
  const removeMember = useRemoveMember(workspace.id)
  const transferOwnership = useTransferOwnership(workspace.id)
  const users = membersQuery.data ?? EMPTY_USERS
  const pendingInvitations = invitations.data?.items.filter((invitation) => invitation.status === 'pending') ?? []

  const [search, setSearch] = useState('')
  const [roleFilter, setRoleFilter] = useState<'all' | Role>('all')
  const [sortBy, setSortBy] = useState<Sort>('name_asc')
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(10)

  const [inviteEmail, setInviteEmail] = useState('')
  const [inviteRole, setInviteRole] = useState<(typeof INVITABLE_ROLES)[number]>('Member')
  const [inviteLink, setInviteLink] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [copyError, setCopyError] = useState(false)

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase()
    const list = users.filter((u) => {
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
  }, [users, search, roleFilter, sortBy])

  if (membersQuery.isPending) return <SettingsCard title="Members"><p>Loading workspace members…</p></SettingsCard>
  if (membersQuery.isError) return <SettingsCard title="Members"><p role="alert">Workspace members could not be loaded. No mock data was substituted.</p></SettingsCard>

  const lastPage = Math.max(1, Math.ceil(filtered.length / perPage))
  const currentPage = Math.min(page, lastPage)
  const firstRow = filtered.length === 0 ? 0 : (currentPage - 1) * perPage + 1
  const lastRow = Math.min(currentPage * perPage, filtered.length)
  const visible = filtered.slice((currentPage - 1) * perPage, currentPage * perPage)

  const generateLink = async (e: React.FormEvent) => {
    e.preventDefault()
    const email = inviteEmail.trim()
    if (!email) return
    try {
      const response = await createInvitation.mutateAsync({ email, role: inviteRole.toLowerCase() as 'admin' | 'member', delivery: 'manual' })
      setInviteLink(response.url ?? null)
      setCopied(false)
    } catch {
      // Mutation feedback remains visible in the form.
    }
  }

  const copyLink = async () => {
    if (!inviteLink) return
    try {
      await navigator.clipboard.writeText(inviteLink)
      setCopied(true)
      setCopyError(false)
    } catch {
      setCopyError(true)
    }
  }

  return (
    <>
      <SettingsCard title="Members" flush>
        <div className="flex items-center justify-between gap-2 border-b border-border p-3 max-[899px]:flex-col max-[899px]:items-stretch">
          <div className="relative w-full max-w-96 max-[899px]:max-w-none">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 z-[1] size-3.5 -translate-y-1/2 text-muted-foreground/70" />
            <Input
              type="search"
              className="h-8 rounded-lg border-border bg-foreground/[0.02] px-8 text-xs"
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
                className="absolute top-1/2 right-2 flex size-5 -translate-y-1/2 items-center justify-center rounded text-muted-foreground/70 transition-colors hover:bg-sidebar-accent/50 hover:text-foreground"
                aria-label="Clear search"
                onClick={() => {
                  setSearch('')
                  setPage(1)
                }}
              >
                <X className="size-3" />
              </button>
            ) : null}
          </div>
          <div className="flex gap-2 max-[899px]:flex-col [&>*]:min-w-0 [&>*:first-child]:w-36 [&>*:last-child]:w-40 max-[899px]:[&>*]:w-full">
            <Listbox<'all' | Role>
              aria-label="Filter by role"
              value={roleFilter}
              options={[{ value: 'all', label: 'All roles' }, ...ROLES.map((role) => ({ value: role, label: role }))]}
              onChange={(value) => {
                setRoleFilter(value)
                setPage(1)
              }}
            />
            <Listbox<Sort>
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
            <div className={TABLE}>
              <div className={TABLE_HEADER}>
                <span>Name</span>
                <span>Email</span>
                <span>Role</span>
                <span className="text-right">Actions</span>
              </div>
              {visible.map((user) => (
                <div key={user.id} data-member-row className={TABLE_ROW}>
                  <div className="flex min-w-0 items-center gap-2">
                    <span className={AVATAR_TILE}>{initial(user)}</span>
                    <span className="truncate text-[13px] font-medium text-foreground">{user.name}</span>
                    {user.id === currentUser.data?.id ? (
                      <span className={cn(BADGE, 'bg-primary/10 text-primary')} data-tone="accent">
                        You
                      </span>
                    ) : null}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">{user.email}</div>
                  <div>
                    <span className={BADGE}>{user.role}</span>
                  </div>
                  <div className="flex justify-end">
                    {canManage && canManageMember(workspace.role, currentUser.data?.id, user) ? (
                      <Dropdown
                        align="right"
                        trigger={(open) => (
                          <button
                            type="button"
                            className={cn(buttonVariants({ variant: 'outline', size: 'sm' }), 'text-[11px]')}
                            aria-haspopup="menu"
                            aria-expanded={open}
                          >
                            Manage
                          </button>
                        )}
                      >
                        {(close) => (
                          <>
                            {INVITABLE_ROLES.filter((role) => role !== user.role).map((role) => (
                              <button
                                key={role}
                                type="button"
                                className={POPOVER_OPTION}
                                onClick={() => {
                                  changeRole.mutate({ membershipId: user.membershipId, version: user.version, role: role.toLowerCase() as 'admin' | 'member' })
                                  close()
                                }}
                              >
                                Make {role.toLowerCase()}
                              </button>
                            ))}
                            {canTransferOwnership(workspace.role, currentUser.data?.id, user) ? (
                              <button type="button" className={POPOVER_OPTION} onClick={async () => {
                                close()
                                if (await confirmAction({ title: `Transfer ownership of ${workspace.name} to ${user.name}?`, confirmLabel: 'Transfer ownership', danger: true })) {
                                  transferOwnership.mutate({ membershipId: user.membershipId, membershipVersion: user.version, workspaceVersion: workspace.version })
                                }
                              }}>Transfer ownership</button>
                            ) : null}
                            <div className="my-1 h-px shrink-0 bg-border" />
                            <button
                              type="button"
                              className={cn(POPOVER_OPTION, 'text-destructive')}
                              data-tone="danger"
                              onClick={() => {
                                removeMember.mutate({ membershipId: user.membershipId, version: user.version })
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
            <footer className="flex min-h-11 items-center justify-between border-t border-border px-4 text-[11px] text-muted-foreground/70">
              <div className="inline-flex h-7 items-center gap-3 whitespace-nowrap tabular-nums [&>button]:h-7 [&>button]:w-16">
                <span>
                  {firstRow}-{lastRow} of {filtered.length}
                </span>
                <Listbox
                  aria-label="Items per page"
                  value={String(perPage)}
                  options={PAGE_SIZES.map((size) => ({ value: String(size), label: String(size) }))}
                  onChange={(value) => {
                    setPerPage(Number(value))
                    setPage(1)
                  }}
                />
              </div>
              <div className="flex items-center gap-1">
                <Button
                  type="button"
                  variant="outline"
                  size="icon-sm"
                  aria-label="Previous page"
                  disabled={currentPage <= 1}
                  onClick={() => setPage(currentPage - 1)}
                >
                  <ArrowRight className="size-3.5 rotate-180" />
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="icon-sm"
                  aria-label="Next page"
                  disabled={currentPage >= lastPage}
                  onClick={() => setPage(currentPage + 1)}
                >
                  <ArrowRight className="size-3.5" />
                </Button>
              </div>
            </footer>
          </>
        ) : (
          <div className="p-4">
            <EmptyState
              size="sm"
              icon={Users}
              title="No matching members"
              description="Try a different name, email address, or role."
            />
          </div>
        )}
        {changeRole.isError ? <p role="alert" className="text-destructive">Role change failed. <Button variant="ghost" onClick={() => changeRole.variables && changeRole.mutate(changeRole.variables)}>Retry</Button></p> : null}
        {removeMember.isError ? <p role="alert" className="text-destructive">Member removal failed. <Button variant="ghost" onClick={() => removeMember.variables && removeMember.mutate(removeMember.variables)}>Retry</Button></p> : null}
        {transferOwnership.isError ? <p role="alert" className="text-destructive">Ownership transfer failed. <Button variant="ghost" onClick={() => transferOwnership.variables && transferOwnership.mutate(transferOwnership.variables)}>Retry</Button></p> : null}
        {changeRole.isPending || removeMember.isPending || transferOwnership.isPending ? <p role="status">Saving member change…</p> : null}
      </SettingsCard>

      {canManage ? (
        <form id="invitations" onSubmit={(event) => void generateLink(event)}>
          <SettingsCard
            title="Invite a member"
            description="Create a reusable invitation link or deliver it by email."
            actions={
              <>
                <Button type="button" variant="outline" disabled title="Email delivery is not configured">
                  <Bell className="size-3.5" />
                  Send email
                </Button>
                <Button type="submit" disabled={createInvitation.isPending}>
                  <Plus className="size-3.5" />
                  Generate link
                </Button>
              </>
            }
          >
            <div className={GRID}>
              <div className={FIELD}>
                <Label className={FIELD_LABEL} htmlFor="invite-email">
                  Email address <span className={REQ}>*</span>
                </Label>
                <Input
                  id="invite-email"
                  type="email"
                  placeholder="teammate@example.com"
                  required
                  value={inviteEmail}
                  onChange={(e) => setInviteEmail(e.target.value)}
                />
              </div>
              <div className={FIELD}>
                <Label className={FIELD_LABEL} htmlFor="invite-role">
                  Role
                </Label>
                <Listbox<(typeof INVITABLE_ROLES)[number]>
                  id="invite-role"
                  value={inviteRole}
                  options={INVITABLE_ROLES.map((role) => ({
                    value: role,
                    label: role,
                  }))}
                  onChange={setInviteRole}
                />
              </div>
            </div>
            {inviteLink ? (
              <div className="mt-4 flex items-end gap-2">
                <div className={FIELD}>
                  <Label className={FIELD_LABEL} htmlFor="invite-link">
                    Invitation link ({inviteRole})
                  </Label>
                  <Input id="invite-link" value={inviteLink} readOnly />
                </div>
                <Button type="button" variant="outline" onClick={copyLink}>
                  <Copy className="size-3.5" />
                  {copied ? 'Copied' : 'Copy'}
                </Button>
              </div>
            ) : null}
            {createInvitation.isError ? <p role="alert" className="text-destructive">The invitation could not be created.</p> : null}
            {copyError ? <p role="alert" className="text-destructive">The invitation link could not be copied. Select and copy it manually.</p> : null}
          </SettingsCard>
        </form>
      ) : null}

      {canManage && invitations.isPending ? <SettingsCard title="Pending invitations"><p role="status">Loading invitations…</p></SettingsCard> : null}
      {canManage && invitations.isError ? <SettingsCard title="Pending invitations"><p role="alert">Invitations could not be loaded. <Button variant="ghost" onClick={() => void invitations.refetch()}>Retry</Button></p></SettingsCard> : null}
      {canManage && pendingInvitations.length > 0 ? (
        <SettingsCard title="Pending invitations" description="Invitation links that have not been accepted." flush>
          <div className={TABLE}>
            {pendingInvitations.map((invitation) => <div className={TABLE_ROW} key={invitation.id}>
              <span className="truncate">{invitation.email}</span><span className="text-xs text-muted-foreground">Expires {new Date(invitation.expires_at).toLocaleDateString()}</span><span className={BADGE}>{invitation.role}</span>
              <span className="text-right"><Button variant="ghost" disabled={revokeInvitation.isPending} onClick={() => revokeInvitation.mutate(invitation.id)}>Revoke</Button></span>
            </div>)}
          </div>
        </SettingsCard>
      ) : null}
      {revokeInvitation.isError ? <p role="alert" className="text-destructive">Invitation revocation failed. <Button variant="ghost" onClick={() => revokeInvitation.variables && revokeInvitation.mutate(revokeInvitation.variables)}>Retry</Button></p> : null}
    </>
  )
}
