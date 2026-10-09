import { useState } from 'react'
import { toast } from 'sonner'
import { Copy } from 'reicon-react'
import type { AdminUser, RecoveryLink } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { useAdminUsers, useCreateRecoveryLink, useResetTwoFactor, useSetInstanceAdmin, useSetSuspension } from '@/features/admin/api'
import { useCurrentUser } from '@/features/auth/api'
import { useIsRoot } from '@/features/workspaces/permissions'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useDebouncedValue } from '@/lib/useDebouncedValue'

function failure(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.problem.detail : fallback
}

/**
 * Accounts on the instance, searched and paged on the server. Nobody manages the root account or their own account
 * here; only the root user manages instance admins (the server enforces the same rules).
 */
export function AdminUsersPage() {
  const suspension = useSetSuspension()
  const recovery = useCreateRecoveryLink()
  const instanceAdmin = useSetInstanceAdmin()
  const twoFactorReset = useResetTwoFactor()
  const viewerIsRoot = useIsRoot()
  const viewerId = useCurrentUser().data?.id
  const [link, setLink] = useState<{ user: AdminUser; link: RecoveryLink } | null>(null)
  const [query, setQuery] = useState('')
  const search = useDebouncedValue(query.trim(), 250)
  const users = useAdminUsers(search)

  async function suspend(user: AdminUser) {
    const confirmed = await confirmAction({
      title: `Suspend ${user.display_name}?`,
      description: 'They are signed out everywhere and cannot sign in until you reinstate the account. Their API tokens stop working.',
      confirmLabel: 'Suspend account',
      danger: true,
    })
    if (!confirmed) return
    suspension.mutate({ userId: user.id, suspended: true }, {
      onSuccess: () => toast(`${user.display_name} is suspended`),
      onError: (error) => toast.error(failure(error, 'The account could not be suspended.')),
    })
  }

  function reinstate(user: AdminUser) {
    suspension.mutate({ userId: user.id, suspended: false }, {
      onSuccess: () => toast(`${user.display_name} is reinstated`),
      onError: (error) => toast.error(failure(error, 'The account could not be reinstated.')),
    })
  }

  async function setAdmin(user: AdminUser, admin: boolean) {
    if (admin) {
      const confirmed = await confirmAction({
        title: `Make ${user.display_name} an instance admin?`,
        description: 'They can manage accounts (except you and other admins), email, registration, backups and the audit log.',
        confirmLabel: 'Make admin',
      })
      if (!confirmed) return
    }
    instanceAdmin.mutate({ userId: user.id, admin }, {
      onSuccess: () => toast(admin ? `${user.display_name} is now an admin` : `${user.display_name} is no longer an admin`),
      onError: (error) => toast.error(failure(error, 'The admin role could not be changed.')),
    })
  }

  async function resetTwoFactor(user: AdminUser) {
    const confirmed = await confirmAction({
      title: `Reset two-factor sign-in for ${user.display_name}?`,
      description: 'Their authenticator app and recovery codes stop working, and they sign in with only their password until they set it up again. Do this only after you have confirmed who is asking.',
      confirmLabel: 'Reset two-factor',
      danger: true,
    })
    if (!confirmed) return
    twoFactorReset.mutate(user.id, {
      onSuccess: () => toast(`Two-factor sign-in is off for ${user.display_name}`),
      onError: (error) => toast.error(failure(error, 'Two-factor sign-in could not be reset.')),
    })
  }

  function createLink(user: AdminUser) {
    recovery.mutate(user.id, {
      onSuccess: (created) => setLink({ user, link: created }),
      onError: (error) => toast.error(failure(error, 'The recovery link could not be created.')),
    })
  }

  return (
    <>
      <SettingsCard
        title="Users"
        description="Every account on this Orbit instance."
        actions={<Input type="search" aria-label="Search users" placeholder="Search by name or email…" value={query} onChange={(event) => setQuery(event.target.value)} className="h-8 w-60 max-sm:w-40" />}
        flush
      >
        <div className="flex flex-col divide-y text-[13px]">
          {users.isPending ? <SettingsRow role="status">Loading users…</SettingsRow> : null}
          {users.isError ? <SettingsRow role="alert">Users could not be loaded. <Button variant="ghost" onClick={() => void users.refetch()}>Retry</Button></SettingsRow> : null}
          {search && users.data?.length === 0 ? <SettingsRow role="status">No user matches "{search}".</SettingsRow> : null}
          {users.data?.map((user) => (
            <SettingsRow key={user.id} className="flex-nowrap" data-admin-user={user.id}>
              <Avatar size="sm" className="size-8">
                <AvatarFallback className="rounded-lg text-[11px] font-semibold">{(user.display_name || user.email).charAt(0).toUpperCase()}</AvatarFallback>
              </Avatar>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex min-w-0 items-center gap-2 font-medium text-foreground">
                  <span className="truncate">{user.display_name}</span>
                  {user.root ? <Badge>Root</Badge> : user.admin ? <Badge variant="secondary">Admin</Badge> : null}
                  {user.two_factor ? <Badge variant="outline">2FA</Badge> : null}
                  {user.suspended ? <Badge variant="destructive">Suspended</Badge> : null}
                </span>
                <span className="truncate text-xs text-muted-foreground">{user.email}</span>
              </div>
              <span className="text-xs text-muted-foreground/70 max-[899px]:hidden">
                {user.workspace_count} {user.workspace_count === 1 ? 'workspace' : 'workspaces'} · {user.last_active_at ? `Active ${new Date(user.last_active_at).toLocaleDateString()}` : 'Never signed in'}
              </span>
              <div className="flex w-24 justify-end">
                {user.root || user.id === viewerId || (user.admin && !viewerIsRoot) ? null : (
                  <DropdownMenu>
                    <DropdownMenuTrigger render={<Button variant="outline" size="sm" className="text-[11px]" aria-label={`Manage ${user.display_name}`} />}>
                      Manage
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="w-auto min-w-52">
                      {viewerIsRoot && user.admin ? <DropdownMenuItem onClick={() => void setAdmin(user, false)}>Remove admin</DropdownMenuItem> : null}
                      {viewerIsRoot && !user.admin && !user.suspended ? <DropdownMenuItem onClick={() => void setAdmin(user, true)}>Make admin</DropdownMenuItem> : null}
                      {user.suspended ? (
                        <DropdownMenuItem onClick={() => reinstate(user)}>Reinstate account</DropdownMenuItem>
                      ) : (
                        <>
                          <DropdownMenuItem onClick={() => createLink(user)}>Create password recovery link</DropdownMenuItem>
                          {user.two_factor ? <DropdownMenuItem onClick={() => void resetTwoFactor(user)}>Reset two-factor</DropdownMenuItem> : null}
                          <DropdownMenuSeparator />
                          {/* data-danger (not variant="destructive"): the menu popup forces destructive items to the accent colour */}
                          <DropdownMenuItem
                            className="text-destructive focus:bg-destructive/10 focus:text-destructive focus:**:text-destructive"
                            data-danger="true"
                            onClick={() => void suspend(user)}
                          >
                            Suspend account
                          </DropdownMenuItem>
                        </>
                      )}
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </div>
            </SettingsRow>
          ))}
          {users.hasNextPage ? (
            <SettingsRow className="justify-center">
              <Button variant="ghost" disabled={users.isFetchingNextPage} onClick={() => void users.fetchNextPage()}>
                {users.isFetchingNextPage ? 'Loading…' : 'Load more users'}
              </Button>
            </SettingsRow>
          ) : null}
        </div>
      </SettingsCard>
      {link ? <RecoveryLinkDialog user={link.user} link={link.link} onClose={() => setLink(null)} /> : null}
    </>
  )
}

function RecoveryLinkDialog({ user, link, onClose }: { user: AdminUser; link: RecoveryLink; onClose: () => void }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(link.url)
      setCopied(true)
      toast('Recovery link copied')
    } catch {
      toast.error('Could not copy the link. Select and copy it manually.')
    }
  }
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Recovery link for {user.display_name}</DialogTitle>
          <DialogDescription>
            Send this link to {user.email}. It sets a new password, works once, and expires at {new Date(link.expires_at).toLocaleTimeString()}.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2">
          <Input aria-label="Recovery link" value={link.url} readOnly onFocus={(event) => event.target.select()} />
          <Button type="button" variant="outline" onClick={() => void copy()}>
            <Copy className="size-3.5" />
            {copied ? 'Copied' : 'Copy'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
