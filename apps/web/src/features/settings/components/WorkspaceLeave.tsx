import { Link, useNavigate } from 'react-router'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { useCurrentUser } from '@/features/auth/api'
import { useLeaveWorkspace, useMembers } from '@/features/workspaces/api'
import { useBillingEnabled } from '@/features/workspaces/billing'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** Any member may leave a workspace; its owner must transfer ownership first (`can_remove` on the caller's own row). */
export function WorkspaceLeave() {
  const { workspace, workspaces } = useWorkspace()
  const userId = useCurrentUser().data?.id
  const members = useMembers(workspace.id)
  const leave = useLeaveWorkspace(workspace.id)
  const navigate = useNavigate()
  const membership = members.data?.find((member) => member.id === userId)
  // Orbit Cloud refuses ownership transfer.
  const billing = useBillingEnabled()

  // The server allows everyone but the owner to leave.
  if (membership && !membership.can.remove) return (
    <SettingsCard title="Leave workspace">
      {billing ? (
        <p className="text-[0.8125rem] text-muted-foreground">
          The owner cannot leave. Ownership transfer is not available on Orbit Cloud yet. To remove the workspace, delete it in <Link to="/settings/danger-zone" className="text-foreground underline underline-offset-4">Danger zone</Link>.
        </p>
      ) : (
        <p className="text-[0.8125rem] text-muted-foreground">
          To leave, first transfer ownership to another member in <Link to="/settings/members" className="text-foreground underline underline-offset-4">Members</Link>, or delete the workspace in <Link to="/settings/danger-zone" className="text-foreground underline underline-offset-4">Danger zone</Link>.
        </p>
      )}
    </SettingsCard>
  )

  async function leaveConfirmed() {
    if (!membership || leave.isPending) return
    const confirmed = await confirmAction({
      title: `Leave ${workspace.name}?`,
      description: 'You lose access to its projects, tasks, docs and chat. To come back, someone must invite you again.',
      confirmLabel: 'Leave workspace',
      danger: true,
    })
    if (!confirmed) return
    try {
      await leave.mutateAsync(membership)
    } catch {
      return // The card shows the error.
    }
    // With no other workspace, `WorkspaceProvider` opens the onboarding (or Admin, for an operator).
    const next = workspaces.find((item) => item.id !== workspace.id)
    if (next) navigate(`/tasks?${new URLSearchParams({ workspace: next.id })}`, { replace: true })
  }

  return (
    <SettingsCard title="Leave workspace">
      <div className="flex flex-col items-start gap-2">
        <Button type="button" variant="destructive" disabled={!membership || leave.isPending} onClick={() => void leaveConfirmed()}>
          {leave.isPending ? 'Leaving…' : 'Leave workspace'}
        </Button>
        {leave.isError ? <p role="alert" className="text-[0.8125rem] text-destructive">You could not leave the workspace. Refresh the page and try again.</p> : null}
      </div>
    </SettingsCard>
  )
}
