import { useId, useState } from 'react'
import { toast } from 'sonner'
import { Copy } from 'reicon-react'
import type { AdminUser } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useAdminUsers, useChangeUserEmail, useCreateRecoveryLink, useResetTwoFactor, useSetInstanceAdmin, useSetSuspension } from '@/features/admin/api'
import { useAuthOptions, useCurrentUser } from '@/features/auth/api'
import { useIsRoot } from '@/features/workspaces/permissions'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useDebouncedValue } from '@/lib/useDebouncedValue'

function failure(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.problem.detail : fallback
}

/** Orbit Cloud emails the person about a changed email, a password reset and a two-factor reset, so they need a mail server. */
const NO_MAIL_SERVER = 'Needs a mail server on Orbit Cloud'

function MenuItemReason({ children }: { children: string }) {
  return <span className="block text-xs text-muted-foreground">{children}</span>
}

/**
 * Accounts on the instance, searched and paged on the server. Nobody manages the root account, their own account or
 * a deleted one here, except that an admin may change their own email; only the root user manages instance admins
 * (the server enforces the same rules).
 */
export function AdminUsersPage() {
  const suspension = useSetSuspension()
  const recovery = useCreateRecoveryLink()
  const instanceAdmin = useSetInstanceAdmin()
  const twoFactorReset = useResetTwoFactor()
  const viewerIsRoot = useIsRoot()
  const viewerId = useCurrentUser().data?.id
  const options = useAuthOptions().data
  const cloud = options?.billing_enabled === true
  const mailBlocked = cloud && !options.email_enabled
  const [link, setLink] = useState<{ user: AdminUser; url: string; expiresAt: string } | null>(null)
  const [emailUser, setEmailUser] = useState<AdminUser | null>(null)
  const [query, setQuery] = useState('')
  const manageable = (user: AdminUser) => !user.deleted && !user.root && user.id !== viewerId && !(user.admin && !viewerIsRoot)
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
      description: `Their authenticator app and recovery codes stop working, and they sign in with only their password until they set it up again.${cloud ? ' They get an email about it.' : ''} Do this only after you have confirmed who is asking.`,
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
      onSuccess: (created) => {
        if (created.url) setLink({ user, url: created.url, expiresAt: created.expires_at })
        else toast('Password reset email sent', { description: `Sent to ${created.sent_to ?? user.email}` })
      },
      onError: (error) => toast.error(failure(error, cloud ? 'The password reset email could not be sent.' : 'The recovery link could not be created.')),
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
                  {user.deleted ? <Badge variant="secondary">Deleted</Badge> : (
                    <>
                      {user.root ? <Badge>Root</Badge> : user.admin ? <Badge variant="secondary">Admin</Badge> : null}
                      {user.two_factor ? <Badge variant="outline">2FA</Badge> : null}
                      {user.suspended ? <Badge variant="destructive">Suspended</Badge> : null}
                    </>
                  )}
                </span>
                {/* a deleted account's email is a placeholder that frees the address */}
                <span className="truncate text-xs text-muted-foreground">{user.deleted ? 'Anonymized. It can never sign in again.' : user.email}</span>
              </div>
              <span className="text-xs text-muted-foreground/70 max-[899px]:hidden">
                {user.workspace_count} {user.workspace_count === 1 ? 'workspace' : 'workspaces'} · {user.last_active_at ? `Active ${new Date(user.last_active_at).toLocaleDateString()}` : 'Never signed in'}
              </span>
              <div className="flex w-24 justify-end">
                {user.deleted || (user.id !== viewerId && !manageable(user)) ? null : (
                  <DropdownMenu>
                    <DropdownMenuTrigger render={<Button variant="outline" size="sm" className="text-[11px]" aria-label={`Manage ${user.display_name}`} />}>
                      Manage
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="w-auto min-w-52">
                      <DropdownMenuItem disabled={mailBlocked} onClick={() => setEmailUser(user)}>
                        <span>Change email{mailBlocked ? <MenuItemReason>{NO_MAIL_SERVER}</MenuItemReason> : null}</span>
                      </DropdownMenuItem>
                      {manageable(user) ? (
                        <>
                          {viewerIsRoot && user.admin ? <DropdownMenuItem onClick={() => void setAdmin(user, false)}>Remove admin</DropdownMenuItem> : null}
                          {viewerIsRoot && !user.admin && !user.suspended ? <DropdownMenuItem onClick={() => void setAdmin(user, true)}>Make admin</DropdownMenuItem> : null}
                          {user.suspended ? (
                            <DropdownMenuItem onClick={() => reinstate(user)}>Reinstate account</DropdownMenuItem>
                          ) : (
                            <>
                              <DropdownMenuItem disabled={mailBlocked} onClick={() => createLink(user)}>
                                <span>{cloud ? 'Send password reset email' : 'Create password recovery link'}{mailBlocked ? <MenuItemReason>{NO_MAIL_SERVER}</MenuItemReason> : null}</span>
                              </DropdownMenuItem>
                              {user.two_factor ? (
                                <DropdownMenuItem disabled={mailBlocked} onClick={() => void resetTwoFactor(user)}>
                                  <span>Reset two-factor{mailBlocked ? <MenuItemReason>{NO_MAIL_SERVER}</MenuItemReason> : null}</span>
                                </DropdownMenuItem>
                              ) : null}
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
                        </>
                      ) : null}
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
      {link ? <RecoveryLinkDialog user={link.user} url={link.url} expiresAt={link.expiresAt} onClose={() => setLink(null)} /> : null}
      {emailUser ? <ChangeEmailDialog user={emailUser} cloud={cloud} onClose={() => setEmailUser(null)} /> : null}
    </>
  )
}

/** Self-hosted only: Orbit Cloud emails the link instead of showing it. */
function RecoveryLinkDialog({ user, url, expiresAt, onClose }: { user: AdminUser; url: string; expiresAt: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url)
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
            Send this link to {user.email}. It sets a new password, works once, and expires at {new Date(expiresAt).toLocaleTimeString()}.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2">
          <Input aria-label="Recovery link" value={url} readOnly onFocus={(event) => event.target.select()} />
          <Button type="button" variant="outline" onClick={() => void copy()}>
            <Copy className="size-3.5" />
            {copied ? 'Copied' : 'Copy'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** A 422 or a taken address is about the field; the rest shows under the form. */
function ChangeEmailDialog({ user, cloud, onClose }: { user: AdminUser; cloud: boolean; onClose: () => void }) {
  const change = useChangeUserEmail()
  const [email, setEmail] = useState('')
  const emailId = useId()
  const problem = change.error instanceof ApiProblem ? change.error : null
  const fieldError = problem !== null && (problem.status === 422 || problem.code === 'email_taken')
  const detail = problem?.detail ?? 'The email could not be changed.'

  return (
    <Modal
      title={`Change email of ${user.display_name}`}
      description={cloud ? 'The person is signed out everywhere. The old and the new address get an email about the change.' : 'The person is signed out everywhere. No confirmation email is sent.'}
      onClose={onClose}
      dismissible={!change.isPending}
      className="sm:max-w-120"
    >
      <form className="grid gap-4" onSubmit={(event) => {
        event.preventDefault()
        change.mutate({ userId: user.id, email: email.trim() }, {
          onSuccess: () => {
            toast(`The email of ${user.display_name} is now ${email.trim()}`)
            onClose()
          },
        })
      }}>
        <FieldGroup>
          <Field data-invalid={fieldError || undefined}>
            <FieldLabel htmlFor={emailId}>New email</FieldLabel>
            <Input id={emailId} type="email" required autoFocus autoComplete="off" aria-invalid={fieldError || undefined} value={email} onChange={(event) => setEmail(event.target.value)} />
            {fieldError ? <FieldError>{detail}</FieldError> : <FieldDescription>Now {user.email}.</FieldDescription>}
          </Field>
        </FieldGroup>
        {change.error && !fieldError ? <p role="alert" className="text-[13px] text-destructive">{detail}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="ghost" disabled={change.isPending} />}>Cancel</DialogClose>
          <Button type="submit" disabled={change.isPending || !email.trim()}>{change.isPending ? 'Saving…' : 'Change email'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
