import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import type { Teamspace } from '@orbit/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { SettingsCard } from '@/components/common/SettingsCard'
import { usePageTree } from '@/features/docs/api/pages'
import { useCreateTeamspace, useTeamspaces, useUpdateTeamspace } from '@/features/docs/api/teamspaces'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import type { Project } from '@/features/tasks/api/models'

const NONE = 'none'

/** The teamspace that holds the project's docs (#64): pick one, make one, or open it. */
export function ProjectDocsCard({ project }: { project: Project }) {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const teamspaces = useTeamspaces(workspace.id)
  const tree = usePageTree(workspace.id)
  const createTeamspace = useCreateTeamspace(workspace.id)
  const updateTeamspace = useUpdateTeamspace(workspace.id)
  const all = teamspaces.data ?? []
  const linked = all.find((teamspace) => teamspace.project_id === project.id)
  // A teamspace of another project is not offered: a project has one, and a teamspace holds one project.
  const choices = all.filter((teamspace) => !teamspace.project_id || teamspace.project_id === project.id)
  const items = [{ value: NONE, label: 'No docs teamspace' }, ...choices.map((teamspace) => ({ value: teamspace.id, label: teamspace.name }))]
  const firstPage = linked ? tree.data?.find((page) => page.teamspace_id === linked.id && !page.parent_id) : undefined
  const pending = createTeamspace.isPending || updateTeamspace.isPending

  const link = (teamspace: Teamspace, projectId: string | null) =>
    updateTeamspace.mutateAsync({ teamspaceId: teamspace.id, version: teamspace.version, projectId })

  const choose = async (value: string) => {
    if (value === (linked?.id ?? NONE)) return
    try {
      if (linked) await link(linked, null)
      const next = choices.find((teamspace) => teamspace.id === value)
      if (next) await link(next, project.id)
    } catch {
      toast.error('Could not change the docs teamspace.')
    }
  }

  const create = async () => {
    try {
      const teamspace = await createTeamspace.mutateAsync({ name: project.name.slice(0, 100) })
      await link(teamspace, project.id)
    } catch {
      toast.error('Could not create the docs teamspace.')
    }
  }

  return (
    <SettingsCard
      title="Docs"
      description="The teamspace that holds this project's docs. Teamspaces without a project hold global docs. An AI client can list the global docs and this project's docs, without the docs of other projects."
    >
      <div className="flex flex-wrap items-center gap-2">
        <Select items={items} value={linked?.id ?? NONE} disabled={pending || !teamspaces.data} onValueChange={(value) => value && void choose(value)}>
          <SelectTrigger aria-label="Docs teamspace" className="w-64 max-w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {items.map((item) => (
              <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        {linked ? (
          <Button variant="outline" disabled={!firstPage} onClick={() => firstPage && navigate(`/docs/${firstPage.id}`)}>
            Open docs
          </Button>
        ) : (
          <Button variant="outline" disabled={pending || !teamspaces.data} onClick={() => void create()}>
            Create docs teamspace
          </Button>
        )}
      </div>
      {linked && tree.data && !firstPage ? <p className="mt-2 text-xs text-muted-foreground">This teamspace has no pages yet. Add one in Docs.</p> : null}
    </SettingsCard>
  )
}
