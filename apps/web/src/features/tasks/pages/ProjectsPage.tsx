import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { Add as Plus, Folder, Setting2 as Settings } from 'reicon-react'
import { ColorDot } from '@/components/common/ColorDot'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Tip } from '@/components/common/Tip'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { useMilestones } from '@/features/tasks/api/milestones'
import { useProjects } from '@/features/tasks/api/projects'
import { HealthDot, TaskProgress } from '@/features/tasks/components/MilestoneBits'
import { NewProjectModal } from '@/features/tasks/components/NewProjectModal'
import { activeMilestone, projectProgress } from '@/features/tasks/roadmap/roadmapLib'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { loadFailed } from '@/lib/connection'

/** Every project of the workspace: who owns it, how far it is, and the milestone it works on now. */
export function ProjectsPage() {
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const projectsQuery = useProjects(workspace.id)
  const milestones = useMilestones(workspace.id).data ?? []
  const members = useMembers(workspace.id).data ?? []
  const [creating, setCreating] = useState(false)
  const projects = projectsQuery.data ?? []

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <PaneTitle render={<h1 />}>Projects</PaneTitle>
          <Button size="sm" className="ml-auto" onClick={() => setCreating(true)}>
            <Plus aria-hidden />
            New project
          </Button>
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {projectsQuery.isPending ? null : loadFailed(projectsQuery) ? (
            <EmptyState icon={Folder} title="Projects unavailable" description="The server could not load the projects." />
          ) : projects.length === 0 ? (
            <EmptyState icon={Folder} title="No projects" description="A project is a product that your team works on." />
          ) : (
            <ul aria-label="Projects">
              {projects.map((project) => {
                const lead = members.find((member) => member.id === project.lead_user_id)
                const team = members.filter((member) => project.member_ids.includes(member.id))
                const active = activeMilestone(milestones.filter((milestone) => milestone.project_id === project.id))
                const progress = projectProgress(project.task_counts)
                return (
                  <li key={project.id} className="group/row flex min-h-11 items-center gap-3 border-b px-4 py-1.5 text-[13px] transition-colors hover:bg-foreground/[0.02]">
                    <ColorDot color={project.color} />
                    <Link to={`/tasks/projects/${project.id}`} className="min-w-0 flex-1 truncate font-medium outline-none hover:underline focus-visible:underline">
                      {project.name}
                    </Link>
                    <span className="w-16 shrink-0 text-xs text-muted-foreground tabular-nums max-[899px]:hidden">{project.key}</span>
                    <span className="flex w-56 min-w-0 shrink-0 items-center gap-1.5 text-xs text-muted-foreground max-[1099px]:hidden">
                      {active ? (
                        <>
                          <HealthDot health={active.health} />
                          <Link to={`/tasks/projects/${project.id}/milestones/${active.id}`} className="truncate outline-none hover:underline focus-visible:underline">{active.name}</Link>
                        </>
                      ) : null}
                    </span>
                    <span className="flex w-16 shrink-0 justify-end max-[640px]:hidden">
                      {progress.total > 0 ? <TaskProgress done={progress.done} total={progress.total} /> : null}
                    </span>
                    <span className="flex w-7 shrink-0 justify-center" title={lead ? `Lead: ${lead.name}` : 'No lead'}>
                      <UserAvatar user={lead} size={20} name={lead ? undefined : '—'} />
                    </span>
                    <span className="flex w-16 shrink-0 justify-end max-[899px]:hidden">
                      {team.length > 0 ? <UserAvatarStack users={team} size={20} /> : null}
                    </span>
                    <Tip label="Project settings" side="left">
                      <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={`${project.name} settings`} onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}>
                        <Settings />
                      </Button>
                    </Tip>
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      </Pane>
      {creating ? (
        <NewProjectModal
          onClose={() => setCreating(false)}
          onCreated={(project) => {
            setCreating(false)
            navigate(`/tasks/projects/${project.id}`)
          }}
        />
      ) : null}
    </div>
  )
}
