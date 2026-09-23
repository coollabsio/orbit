import { useMemo, useState } from 'react'
import { confirmAction } from '@/components/common/confirmAction'
import { ArrowRight, Notification as Bell, Copy, Add as Plus, SearchNormal as Search, People as Users, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { EmptyState } from '@/components/common/EmptyState'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCurrentUser } from '@/features/auth/api'
import { useChangeMemberRole, useCreateInvitation, useInvitations, useMembers, useRemoveMember, useRevokeInvitation, useTransferOwnership } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import type { User } from '@/features/workspaces/models'
import { INVITABLE_ROLES, canManageMember, canTransferOwnership } from '@/features/settings/memberPermissions'
import { SettingsCard } from '@/components/common/SettingsCard'

type Role = User['role']
type Sort = 'name_asc' | 'name_desc' | 'email_asc' | 'role'

const ROLES: Role[] = ['Owner', 'Admin', 'Member']
const PAGE_SIZES = [10, 25, 50, 100]
// `items` lets Select.Value render the option label instead of the raw value.
const ROLE_FILTER_OPTIONS = [{ value: 'all', label: 'All roles' }, ...ROLES.map((role) => ({ value: role, label: role }))]
const SORT_OPTIONS: { value: Sort; label: string }[] = [
  { value: 'name_asc', label: 'Name A–Z' },
  { value: 'name_desc', label: 'Name Z–A' },
  { value: 'email_asc', label: 'Email A–Z' },
  { value: 'role', label: 'Role' },
]
const PAGE_SIZE_OPTIONS = PAGE_SIZES.map((size) => ({ value: String(size), label: String(size) }))
const INVITE_ROLE_OPTIONS = INVITABLE_ROLES.map((role) => ({ value: role, label: role }))
const EMPTY_USERS: User[] = []

const FIELD_LABEL = 'mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground'
const GRID = 'grid grid-cols-1 gap-4 min-[900px]:grid-cols-2'
// `gap-0` keeps the Field rows at the previous label/control spacing (the label owns its `mb-1.5`).
const FIELD = 'w-full min-w-0 gap-0'
const REQ = 'inline-block font-semibold text-primary'
const TABLE = 'flex min-w-0 max-w-full flex-col overflow-x-auto [overscroll-behavior-x:contain]'
const TABLE_GRID = '[grid-template-columns:minmax(0,1.15fr)_minmax(0,1.55fr)_7rem_minmax(10rem,0.9fr)] max-[899px]:[grid-template-columns:minmax(0,1fr)_auto_auto] max-[899px]:[&>:nth-child(2)]:hidden'
const TABLE_HEADER = cn('grid h-10 items-center gap-4 border-b border-muted bg-black/[0.02] px-4 text-[13px] font-medium text-muted-foreground dark:bg-white/[0.02]', TABLE_GRID)
const TABLE_ROW = cn('grid min-h-12 items-center gap-4 border-b border-border px-4 py-2.5 transition-colors last:border-b-0 hover:bg-foreground/[0.02]', TABLE_GRID)
const BADGE = 'h-auto rounded-full border-0 bg-sidebar-accent text-[10px] leading-[14px] text-muted-foreground'
// data-danger (not variant="destructive"): the preset menu popup forces destructive items to the accent color.
const MENU_ITEM = 'min-h-8 w-full justify-start gap-2 rounded-md px-2 py-1.5 text-left text-sm font-normal focus:bg-accent focus:text-accent-foreground data-[danger=true]:text-destructive data-[danger=true]:focus:bg-destructive/10 data-[danger=true]:focus:text-destructive data-[danger=true]:focus:**:text-destructive'
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
          <InputGroup className="w-full max-w-96 rounded-lg border-border bg-foreground/[0.02] max-[899px]:max-w-none">
            <InputGroupAddon align="inline-start">
              <Search className="size-3.5 text-muted-foreground/70" />
            </InputGroupAddon>
            <InputGroupInput
              type="search"
              className="text-xs"
              placeholder="Search members"
              aria-label="Search members"
              value={search}
              onChange={(e) => {
                setSearch(e.target.value)
                setPage(1)
              }}
            />
            {search ? (
              <InputGroupAddon align="inline-end">
                <InputGroupButton
                  size="icon-xs"
                  className="size-5 rounded text-muted-foreground/70 hover:bg-sidebar-accent/50 hover:text-foreground dark:hover:bg-sidebar-accent/50"
                  aria-label="Clear search"
                  onClick={() => {
                    setSearch('')
                    setPage(1)
                  }}
                >
                  <X className="size-3" />
                </InputGroupButton>
              </InputGroupAddon>
            ) : null}
          </InputGroup>
          <div className="flex gap-2 max-[899px]:flex-col">
            <Select
              items={ROLE_FILTER_OPTIONS}
              value={roleFilter}
              onValueChange={(value) => {
                setRoleFilter(value as 'all' | Role)
                setPage(1)
              }}
            >
              <SelectTrigger aria-label="Filter by role" className="w-36 max-[899px]:w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {ROLE_FILTER_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
            <Select
              items={SORT_OPTIONS}
              value={sortBy}
              onValueChange={(value) => {
                setSortBy(value as Sort)
                setPage(1)
              }}
            >
              <SelectTrigger aria-label="Sort members" className="w-40 max-[899px]:w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SORT_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
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
                      <Badge className={cn(BADGE, 'bg-primary/10 text-primary')} data-tone="accent">
                        You
                      </Badge>
                    ) : null}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">{user.email}</div>
                  <div>
                    <Badge className={BADGE}>{user.role}</Badge>
                  </div>
                  <div className="flex justify-end">
                    {canManage && canManageMember(workspace.role, currentUser.data?.id, user) ? (
                      <DropdownMenu>
                        <DropdownMenuTrigger
                          render={<Button variant="outline" size="sm" className="text-[11px]" />}
                        >
                          Manage
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-auto min-w-[13rem] p-1">
                          {INVITABLE_ROLES.filter((role) => role !== user.role).map((role) => (
                            <DropdownMenuItem
                              key={role}
                              className={MENU_ITEM}
                              onClick={() => changeRole.mutate({ membershipId: user.membershipId, version: user.version, role: role.toLowerCase() as 'admin' | 'member' })}
                            >
                              Make {role.toLowerCase()}
                            </DropdownMenuItem>
                          ))}
                          {canTransferOwnership(workspace.role, currentUser.data?.id, user) ? (
                            <DropdownMenuItem className={MENU_ITEM} onClick={async () => {
                              if (await confirmAction({ title: `Transfer ownership of ${workspace.name} to ${user.name}?`, confirmLabel: 'Transfer ownership', danger: true })) {
                                transferOwnership.mutate({ membershipId: user.membershipId, membershipVersion: user.version, workspaceVersion: workspace.version })
                              }
                            }}>Transfer ownership</DropdownMenuItem>
                          ) : null}
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            className={MENU_ITEM}
                            data-danger="true"
                            onClick={() => removeMember.mutate({ membershipId: user.membershipId, version: user.version })}
                          >
                            Remove member
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    ) : null}
                  </div>
                </div>
              ))}
            </div>
            <footer className="flex min-h-11 items-center justify-between border-t border-border px-4 text-[11px] text-muted-foreground/70">
              <div className="inline-flex h-7 items-center gap-3 whitespace-nowrap tabular-nums">
                <span>
                  {firstRow}-{lastRow} of {filtered.length}
                </span>
                <Select
                  items={PAGE_SIZE_OPTIONS}
                  value={String(perPage)}
                  onValueChange={(value) => {
                    setPerPage(Number(value))
                    setPage(1)
                  }}
                >
                  <SelectTrigger aria-label="Items per page" size="sm" className="w-16 px-2">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {PAGE_SIZE_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                  </SelectContent>
                </Select>
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
              <Field className={FIELD}>
                <FieldLabel className={FIELD_LABEL} htmlFor="invite-email">
                  Email address <span className={REQ}>*</span>
                </FieldLabel>
                <Input
                  id="invite-email"
                  type="email"
                  placeholder="teammate@example.com"
                  required
                  value={inviteEmail}
                  onChange={(e) => setInviteEmail(e.target.value)}
                />
              </Field>
              <Field className={FIELD}>
                <FieldLabel className={FIELD_LABEL} htmlFor="invite-role">
                  Role
                </FieldLabel>
                <Select items={INVITE_ROLE_OPTIONS} value={inviteRole} onValueChange={(value) => setInviteRole(value as (typeof INVITABLE_ROLES)[number])}>
                  <SelectTrigger id="invite-role">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {INVITE_ROLE_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                  </SelectContent>
                </Select>
              </Field>
            </div>
            {inviteLink ? (
              <div className="mt-4 flex items-end gap-2">
                <Field className={FIELD}>
                  <FieldLabel className={FIELD_LABEL} htmlFor="invite-link">
                    Invitation link ({inviteRole})
                  </FieldLabel>
                  <Input id="invite-link" value={inviteLink} readOnly />
                </Field>
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
              <span className="truncate">{invitation.email}</span><span className="text-xs text-muted-foreground">Expires {new Date(invitation.expires_at).toLocaleDateString()}</span><Badge className={BADGE}>{invitation.role}</Badge>
              <span className="text-right"><Button variant="ghost" disabled={revokeInvitation.isPending} onClick={() => revokeInvitation.mutate(invitation.id)}>Revoke</Button></span>
            </div>)}
          </div>
        </SettingsCard>
      ) : null}
      {revokeInvitation.isError ? <p role="alert" className="text-destructive">Invitation revocation failed. <Button variant="ghost" onClick={() => revokeInvitation.variables && revokeInvitation.mutate(revokeInvitation.variables)}>Retry</Button></p> : null}
    </>
  )
}
