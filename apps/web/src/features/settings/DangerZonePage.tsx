import { useWorkspace } from '../workspaces/workspaceContext'
import { SettingsCard } from './SettingsCard'
import { WorkspaceDangerZone } from './WorkspaceDangerZone'

export function DangerZonePage() {
  const { workspace } = useWorkspace()
  if (workspace.role !== 'owner') return (
    <SettingsCard title="Danger zone">
      <p>Only the workspace owner can delete this workspace.</p>
    </SettingsCard>
  )
  return <WorkspaceDangerZone key={workspace.id} />
}
