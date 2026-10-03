import { useMemo, useState, type ComponentProps } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { confirmAction } from '@/components/common/confirmAction'
import { ArrowRight, Notification as Bell, Copy, Add as Plus, SearchNormal as Search, People as Users, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { EmptyState } from '@/components/common/EmptyState'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { AvatarStatusBadge } from '@/components/common/UserAvatar'
import { presenceOf, usePresence } from '@/features/realtime/presence'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCurrentUser } from '@/features/auth/api'
import { useChangeMemberRole, useCreateInvitation, useInvitations, useMembers, useRemoveMember, useRevokeInvitation, useTransferOwnership } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import type { User } from '@/features/workspaces/models'
import { SettingsCard } from '@/components/common/SettingsCard'
import { FieldGrid, RequiredMark } from '@/features/settings/components/SettingsParts'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

type Role = User['role']
type Sort = 'name_asc' | 'name_desc' | 'email_asc' | 'role'

const ROLES: Role[] = ['Owner', 'Admin', 'Member']
const PAGE_SIZES = [10, 25, 50, 100]
// Owner is never a choice: that role only moves through an ownership transfer.
const INVITABLE_ROLES = ['Admin', 'Member'] as const
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

function initial(user: User) {
  return (user.name || user.email).charAt(0).toUpperCase()
}

export function MembersPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('members.manage')
  const currentUser = useCurrentUser()
  const presence = usePresence()
  const membersQuery = useMembers(workspace.id)
  const createInvitation = useCreateInvitation(workspace.id)
  const invitations = useInvitations(canManage ? workspace.id : null)
  const revokeInvitation = useRevokeInvitation(workspace.id)
  const changeRole = useChangeMemberRole(workspace.id)
  const removeMember = useRemoveMember(workspace.id)
  const transferOwnership = useTransferOwnership(workspace.id)
  const memberSaving = useSlowPending(changeRole.isPending || removeMember.isPending || transferOwnership.isPending)
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
        <div className="flex items-center justify-between gap-2 border-b p-3 max-[899px]:flex-col max-[899px]:items-stretch">
          <InputGroup className="max-w-96 max-[899px]:max-w-none">
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
                  className="text-muted-foreground"
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
            <MemberTable>
              <MemberRow kind="header">
                <span>Name</span>
                <span>Email</span>
                <span>Role</span>
                <span className="text-right">Actions</span>
              </MemberRow>
              {visible.map((user) => (
                <MemberRow key={user.id} data-member-row>
                  <div className="flex min-w-0 items-center gap-2">
                    <ProfileTrigger userId={user.id} name={user.name} kind="avatar" tabIndex={-1} className="flex shrink-0">
                      <Avatar size="sm" className="size-7">
                        <AvatarFallback className="rounded-lg text-[11px] font-semibold">{initial(user)}</AvatarFallback>
                        <AvatarStatusBadge status={presenceOf(presence, user.id).status} size={28} />
                      </Avatar>
                    </ProfileTrigger>
                    <ProfileTrigger userId={user.id} name={user.name} className="truncate text-[13px] font-medium text-foreground">
                      {user.name}
                    </ProfileTrigger>
                    {user.id === currentUser.data?.id ? (
                      <Badge data-tone="accent">
                        You
                      </Badge>
                    ) : null}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">{user.email}</div>
                  <div>
                    <Badge variant="secondary">{user.role}</Badge>
                  </div>
                  <div className="flex justify-end">
                    {/* Your own row has no menu; the abilities themselves come from the server. */}
                    {currentUser.data && user.id !== currentUser.data.id && (user.can.changeRole || user.can.remove) ? (
                      <DropdownMenu>
                        <DropdownMenuTrigger
                          render={<Button variant="outline" size="sm" className="text-[11px]" />}
                        >
                          Manage
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-auto min-w-52">
                          {(user.can.changeRole ? INVITABLE_ROLES : []).filter((role) => role !== user.role).map((role) => (
                            <DropdownMenuItem
                              key={role}
                              onClick={() => changeRole.mutate({ membershipId: user.membershipId, version: user.version, role: role.toLowerCase() as 'admin' | 'member' })}
                            >
                              Make {role.toLowerCase()}
                            </DropdownMenuItem>
                          ))}
                          {user.can.transferOwnership ? (
                            <DropdownMenuItem onClick={async () => {
                              if (await confirmAction({ title: `Transfer ownership of ${workspace.name} to ${user.name}?`, confirmLabel: 'Transfer ownership', danger: true })) {
                                transferOwnership.mutate({ membershipId: user.membershipId, membershipVersion: user.version, workspaceVersion: workspace.version })
                              }
                            }}>Transfer ownership</DropdownMenuItem>
                          ) : null}
                          {user.can.remove ? (
                            <>
                              <DropdownMenuSeparator />
                              {/* data-danger (not variant="destructive"): the menu popup forces destructive items to the accent colour */}
                              <DropdownMenuItem
                                className="text-destructive focus:bg-destructive/10 focus:text-destructive focus:**:text-destructive"
                                data-danger="true"
                                onClick={() => removeMember.mutate({ membershipId: user.membershipId, version: user.version })}
                              >
                                Remove member
                              </DropdownMenuItem>
                            </>
                          ) : null}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    ) : null}
                  </div>
                </MemberRow>
              ))}
            </MemberTable>
            <footer className="flex min-h-11 items-center justify-between border-t px-4 text-[11px] text-muted-foreground/70">
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
        {memberSaving ? <p role="status">Saving member change…</p> : null}
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
            <FieldGrid>
              <Field>
                <FieldLabel htmlFor="invite-email">
                  Email address <RequiredMark />
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
              <Field>
                <FieldLabel htmlFor="invite-role">
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
            </FieldGrid>
            {inviteLink ? (
              <div className="mt-4 flex items-end gap-2">
                <Field>
                  <FieldLabel htmlFor="invite-link">
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
          <MemberTable>
            {pendingInvitations.map((invitation) => <MemberRow key={invitation.id}>
              <span className="truncate">{invitation.email}</span><span className="text-xs text-muted-foreground">Expires {new Date(invitation.expires_at).toLocaleDateString()}</span><Badge variant="secondary">{invitation.role}</Badge>
              <span className="text-right"><Button variant="ghost" disabled={revokeInvitation.isPending} onClick={() => revokeInvitation.mutate(invitation.id)}>Revoke</Button></span>
            </MemberRow>)}
          </MemberTable>
        </SettingsCard>
      ) : null}
      {revokeInvitation.isError ? <p role="alert" className="text-destructive">Invitation revocation failed. <Button variant="ghost" onClick={() => revokeInvitation.variables && revokeInvitation.mutate(revokeInvitation.variables)}>Retry</Button></p> : null}
    </>
  )
}

/** Scrolls sideways on narrow screens instead of squeezing the columns. */
function MemberTable({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="member-table" className={cn('flex max-w-full min-w-0 flex-col overflow-x-auto [overscroll-behavior-x:contain]', className)} {...props} />
}

/** Name · email · role · actions; below 900px the email column hides. */
const memberRowVariants = cva(
  'grid items-center gap-4 px-4 [grid-template-columns:minmax(0,1.15fr)_minmax(0,1.55fr)_7rem_minmax(10rem,0.9fr)] max-[899px]:[grid-template-columns:minmax(0,1fr)_auto_auto] max-[899px]:[&>:nth-child(2)]:hidden',
  {
    variants: {
      kind: {
        header: 'h-10 border-b border-muted bg-black/[0.02] text-[13px] font-medium text-muted-foreground dark:bg-white/[0.02]',
        row: 'min-h-12 border-b py-2.5 transition-colors last:border-b-0 hover:bg-foreground/[0.02]',
      },
    },
    defaultVariants: { kind: 'row' },
  },
)

function MemberRow({ kind, className, ...props }: ComponentProps<'div'> & VariantProps<typeof memberRowVariants>) {
  return <div data-slot="member-row" data-kind={kind ?? 'row'} className={cn(memberRowVariants({ kind }), className)} {...props} />
}
