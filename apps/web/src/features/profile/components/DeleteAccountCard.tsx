import { useId, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { ApiProblem } from '@orbit/api/problem'
import type { AuthProblem, OwnedWorkspace } from '@orbit/api/generated/types.gen'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useAuthOptions, useCurrentUser, useDeleteAccount } from '@/features/auth/api'
import { CurrentPassword } from '@/features/auth/components/CurrentPassword'

/** The danger card at the end of the profile page. The root account cannot be deleted, so it only says so. */
export function DeleteAccountCard() {
  const me = useCurrentUser().data
  const [open, setOpen] = useState(false)
  if (!me) return null
  return (
    <>
      <SettingsCard
        className="shadow-[0_0_0_1px_var(--destructive)] hover:shadow-[0_0_0_1px_var(--destructive)] [&_h3]:text-destructive [&>div]:bg-transparent [&>div]:pt-0 [&>div]:shadow-none"
        title="Delete account"
        description={me.root
          ? 'The root account cannot be deleted.'
          : 'Delete your account for good. What you wrote stays, without your name.'}
      >
        {me.root ? null : <Button type="button" variant="destructive" onClick={() => setOpen(true)}>Delete account</Button>}
      </SettingsCard>
      {open ? <DeleteAccountDialog email={me.email} hasPassword={me.has_password !== false} onClose={() => setOpen(false)} /> : null}
    </>
  )
}

function DeleteAccountDialog({ email, hasPassword, onClose }: { email: string; hasPassword: boolean; onClose: () => void }) {
  const navigate = useNavigate()
  const deletion = useDeleteAccount()
  const cloud = useAuthOptions().data?.billing_enabled === true
  const [confirmation, setConfirmation] = useState('')
  const [password, setPassword] = useState('')
  const confirmationId = useId()
  const error = deletion.error
  const problem = error instanceof ApiProblem ? error : null
  const owned = problem?.code === 'owns_workspaces' ? ownedWorkspaces(problem) : null
  const matches = confirmation.trim().toLowerCase() === email.toLowerCase()

  return (
    <Modal title="Delete your account?" description="You are signed out everywhere and cannot sign in again. This cannot be undone." onClose={onClose} dismissible={!deletion.isPending} className="sm:max-w-120">
      <form className="grid gap-4" onSubmit={(event) => {
        event.preventDefault()
        if (!matches || deletion.isPending) return
        deletion.mutate({ email: confirmation.trim(), password: hasPassword ? password : null }, {
          onSuccess: () => void navigate('/login', { replace: true }),
        })
      }}>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor={confirmationId}>Confirm your email</FieldLabel>
            <FieldDescription>Type <strong>{email}</strong> to confirm.</FieldDescription>
            <Input id={confirmationId} type="email" autoComplete="off" required autoFocus value={confirmation} readOnly={deletion.isPending} onChange={(event) => setConfirmation(event.target.value)} />
          </Field>
          <CurrentPassword returnTo="/profile" autoFocus={false} hasPassword={hasPassword} value={password} onChange={setPassword} error={problem?.status === 401 ? error : null} fallback="Your account could not be deleted." />
        </FieldGroup>
        {owned ? <OwnedWorkspaces workspaces={owned} cloud={cloud} /> : null}
        {error && !owned && problem?.status !== 401 ? (
          <p role="alert" className="text-[13px] text-destructive">{problem ? problem.detail : 'Your account could not be deleted. Try again.'}</p>
        ) : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="ghost" disabled={deletion.isPending} />}>Cancel</DialogClose>
          <Button type="submit" variant="destructive" disabled={!matches || deletion.isPending || (hasPassword && !password)}>{deletion.isPending ? 'Deleting…' : 'Delete account'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

/** The auth routes answer 409 `owns_workspaces` with the workspaces in the problem body. */
function ownedWorkspaces(problem: ApiProblem): OwnedWorkspace[] {
  const { workspaces } = problem.problem as AuthProblem
  return Array.isArray(workspaces) ? workspaces : []
}

/** 409 `owns_workspaces`: the account still owns these, so they go first. */
function OwnedWorkspaces({ workspaces, cloud }: { workspaces: OwnedWorkspace[]; cloud: boolean }) {
  return (
    <div role="alert" className="grid gap-2 text-[13px]">
      <p className="text-destructive">You own {workspaces.length === 1 ? 'this workspace' : 'these workspaces'}:</p>
      <ul className="grid gap-1 rounded-lg bg-muted px-4 py-3">
        {workspaces.map((workspace) => (
          <li key={workspace.id}>
            <Link to={`/settings/danger-zone?${new URLSearchParams({ workspace: workspace.id })}`} className="font-medium text-foreground underline-offset-4 hover:underline">{workspace.name}</Link>
          </li>
        ))}
      </ul>
      <p className="text-muted-foreground">
        {cloud
          ? 'Delete them first in Workspace settings → Danger zone. Ownership transfer is not available on Orbit Cloud.'
          : 'Transfer them to another member (Workspace settings → Members) or delete them (Workspace settings → Danger zone) first.'}
      </p>
    </div>
  )
}
