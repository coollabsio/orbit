import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SettingsCard } from '@/components/common/SettingsCard'
import { WorkspaceDangerZone } from '@/features/settings/components/WorkspaceDangerZone'

export function DangerZonePage() {
  const { workspace } = useWorkspace()
  if (workspace.role !== 'owner') return (
    <SettingsCard title="Danger zone">
      <p>Only the workspace owner can delete this workspace.</p>
    </SettingsCard>
  )
  return <WorkspaceDangerZone key={workspace.id} />
}
