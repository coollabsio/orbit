import { Link, useNavigate } from 'react-router'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { useCurrentUser } from '@/features/auth/api'
import { useLeaveWorkspace, useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** Any member may leave a workspace; its owner must transfer ownership first (`can_remove` on the caller's own row). */
export function WorkspaceLeave() {
  const { workspace, workspaces } = useWorkspace()
  const userId = useCurrentUser().data?.id
  const members = useMembers(workspace.id)
  const leave = useLeaveWorkspace(workspace.id)
  const navigate = useNavigate()
  const membership = members.data?.find((member) => member.id === userId)

  // The server allows everyone but the owner to leave.
  if (membership && !membership.can.remove) return (
    <SettingsCard title="Leave workspace" description="You own this workspace.">
      <p className="text-[13px] text-muted-foreground">
        To leave, first transfer ownership to another member in <Link to="/settings/members" className="text-foreground underline underline-offset-4">Members</Link>.
      </p>
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
    <SettingsCard title="Leave workspace" description="Remove yourself from this workspace.">
      <div className="flex flex-col items-start gap-2">
        <Button type="button" variant="destructive" disabled={!membership || leave.isPending} onClick={() => void leaveConfirmed()}>
          {leave.isPending ? 'Leaving…' : 'Leave workspace'}
        </Button>
        {leave.isError ? <p role="alert" className="text-[13px] text-destructive">You could not leave the workspace. Refresh the page and try again.</p> : null}
      </div>
    </SettingsCard>
  )
}
