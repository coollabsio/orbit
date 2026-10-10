import { useId, useState } from 'react'
import { cn } from 'cn'
import { Link, useNavigate, useParams } from 'react-router'
import { Add as Plus, ArrowLeft, Chart as ChartIcon, Folder, Setting2 as Settings, Signpost, TaskSquare as SquareCheck } from 'reicon-react'
import { ColorDot } from '@/components/common/ColorDot'
import { EmptyState } from '@/components/common/EmptyState'
import { Modal } from '@/components/common/Modal'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Tip } from '@/components/common/Tip'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { Button, buttonVariants } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { MemberPicker } from '@/features/chat/components/dialogs/MemberPicker'
import { MILESTONE_STATUS_LABEL, useCreateMilestone, useEnsureProjectOverviewPage, useMilestones, type Milestone } from '@/features/tasks/api/milestones'
import { useCycles } from '@/features/tasks/api/cycles'
import type { Project } from '@/features/tasks/api/models'
import { cycleDatesLabel, cycleName } from '@/features/tasks/cyclesLib'
import { useProjects, useUpdateProject } from '@/features/tasks/api/projects'
import { DescriptionEditor } from '@/features/tasks/components/DescriptionEditor'
import { HealthLabel, MilestoneProgress, TaskProgress } from '@/features/tasks/components/MilestoneBits'
import { dayInputToIso, isMilestoneStatus, milestoneDatesLabel, projectProgress } from '@/features/tasks/roadmap/roadmapLib'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** What a project is and who owns it: lead, members, description and milestones. */
export function ProjectOverviewPage() {
  const { projectId } = useParams()
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const projectsQuery = useProjects(workspace.id)
  const project = projectsQuery.data?.find((item) => item.id === projectId)

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <Tip label="All projects" side="bottom">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate('/tasks/projects')} aria-label="All projects">
              <ArrowLeft className="size-4" />
            </Button>
          </Tip>
          {project ? <ColorDot color={project.color} /> : null}
          <PaneTitle render={<h1 />}>{project?.name ?? 'Project'}</PaneTitle>
          {project ? (
            <div className="ml-auto flex items-center gap-1">
              <Link to={`/tasks/projects/${project.id}/insights`} className={buttonVariants({ variant: 'ghost', size: 'sm' })}>
                <ChartIcon aria-hidden />
                Insights
              </Link>
              <Link to={`/tasks?project=${project.id}`} className={buttonVariants({ variant: 'ghost', size: 'sm' })}>
                <SquareCheck aria-hidden />
                Tasks
              </Link>
              <Tip label="Project settings" side="bottom">
                <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Project settings" onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}>
                  <Settings />
                </Button>
              </Tip>
            </div>
          ) : null}
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {projectsQuery.isPending ? null : !project ? (
            <EmptyState icon={Folder} title="Project not found" description="This project does not exist or was removed." />
          ) : (
            <ProjectOverview key={project.id} project={project} />
          )}
        </div>
      </Pane>
    </div>
  )
}

function ProjectOverview({ project }: { project: Project }) {
  const { workspace } = useWorkspace()
  const members = useMembers(workspace.id).data ?? []
  const milestones = (useMilestones(workspace.id).data ?? []).filter((milestone) => milestone.project_id === project.id)
  const updateProject = useUpdateProject(workspace.id, project.id)
  const ensurePage = useEnsureProjectOverviewPage(workspace.id, project.id)
  const [creating, setCreating] = useState(false)
  const lead = members.find((member) => member.id === project.lead_user_id)
  const team = members.filter((member) => project.member_ids.includes(member.id))
  const progress = projectProgress(project.task_counts)
  const cycles = useCycles(workspace.id, project.id).data ?? []
  const currentCycle = cycles.find((cycle) => cycle.state === 'current')

  // PATCH replaces name, key and colour, so they travel with every change
  const save = (people: { lead_user_id?: string | null; member_ids?: string[] }) =>
    updateProject.mutate({ name: project.name, key: project.key, color: project.color, expected_version: project.version, ...people })

  return (
    <div className="mx-auto flex w-full max-w-[800px] flex-col gap-8 px-10 pt-7 pb-12 max-[899px]:px-5">
      <section className="flex flex-wrap items-center gap-x-6 gap-y-2 text-[13px]">
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground/70">Lead</span>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button variant="ghost" size="sm" className="-ml-1 font-normal" aria-label={`Lead: ${lead?.name ?? 'none'}`}>
                  <UserAvatar user={lead} size={18} name={lead ? undefined : '—'} />
                  {lead?.name ?? 'No lead'}
                </Button>
              }
            />
            <DropdownMenuContent className="max-h-72 w-auto min-w-52">
              {members.map((member) => (
                <DropdownMenuItem key={member.id} className="data-selected:bg-accent data-selected:font-medium" data-selected={member.id === lead?.id || undefined} onClick={() => save({ lead_user_id: member.id })}>
                  <UserAvatar user={member} size={18} />
                  {member.name}
                </DropdownMenuItem>
              ))}
              {lead ? <DropdownMenuItem onClick={() => save({ lead_user_id: null })}>No lead</DropdownMenuItem> : null}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground/70">Members</span>
          <MembersButton members={members} team={team} onChange={(ids) => save({ member_ids: ids })} />
        </div>
        {progress.total > 0 ? (
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground/70">Progress</span>
            <TaskProgress done={progress.done} total={progress.total} />
          </div>
        ) : null}
        {updateProject.isError ? <p role="alert" className="w-full text-xs text-destructive">The change was not saved. Try again.</p> : null}
      </section>

      <section aria-label="Description" className="flex flex-col gap-2">
        <h2 className="text-xs font-medium text-muted-foreground/70">Description</h2>
        <DescriptionEditor workspaceId={workspace.id} pageId={project.overview_page_id} onCreate={() => ensurePage.mutateAsync()} />
      </section>

      {cycles.length > 0 ? (
        <section aria-label="Cycles" className="flex flex-col gap-2">
          <div className="flex items-center">
            <h2 className="text-xs font-medium text-muted-foreground/70">Cycle</h2>
            <Link to={`/tasks/projects/${project.id}/cycles`} className={cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'ml-auto')}>All cycles</Link>
          </div>
          {currentCycle ? (
            <div className="flex min-h-10 items-center gap-3 rounded-lg border px-3 py-1.5 text-[13px]">
              <Link to={`/tasks/projects/${project.id}/cycles/${currentCycle.id}`} className="min-w-0 flex-1 truncate font-medium outline-none hover:underline focus-visible:underline">{cycleName(currentCycle)}</Link>
              <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{cycleDatesLabel(currentCycle)}</span>
              <TaskProgress done={currentCycle.scope_points > 0 ? currentCycle.done_points : currentCycle.done_count} total={currentCycle.scope_points > 0 ? currentCycle.scope_points : currentCycle.scope_count} />
            </div>
          ) : (
            <p className="text-[13px] text-muted-foreground">No cycle is current now (a cooldown).</p>
          )}
        </section>
      ) : null}

      <section aria-label="Milestones" className="flex flex-col gap-2">
        <div className="flex items-center">
          <h2 className="text-xs font-medium text-muted-foreground/70">Milestones</h2>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={() => setCreating(true)}>
            <Plus aria-hidden />
            New milestone
          </Button>
        </div>
        {milestones.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">A milestone is a deliverable with dates, for example a release. This project has none.</p>
        ) : (
          <ul className="rounded-lg border">
            {milestones.map((milestone) => <MilestoneRow key={milestone.id} milestone={milestone} />)}
          </ul>
        )}
      </section>
      {creating ? <NewMilestoneModal projectId={project.id} onClose={() => setCreating(false)} /> : null}
    </div>
  )
}

function MembersButton({ members, team, onChange }: { members: User[]; team: User[]; onChange: (ids: string[]) => void }) {
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="sm" className="-ml-1 font-normal" aria-label={`Members: ${team.length}`}>
            {team.length > 0 ? <UserAvatarStack users={team} size={18} max={5} /> : 'Add members'}
          </Button>
        }
      />
      <PopoverContent align="start" className="w-72 p-2">
        <MemberPicker people={members} selectedIds={team.map((member) => member.id)} onChange={onChange} label="Project members" autoFocus />
      </PopoverContent>
    </Popover>
  )
}

function MilestoneRow({ milestone }: { milestone: Milestone }) {
  return (
    <li className="flex min-h-10 items-center gap-3 border-b px-3 py-1.5 text-[13px] last:border-b-0">
      <Signpost aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
      <Link to={`/tasks/projects/${milestone.project_id}/milestones/${milestone.id}`} className="min-w-0 flex-1 truncate font-medium outline-none hover:underline focus-visible:underline">
        {milestone.name}
      </Link>
      <span className="shrink-0 text-xs text-muted-foreground max-[640px]:hidden">{isMilestoneStatus(milestone.status) ? MILESTONE_STATUS_LABEL[milestone.status] : milestone.status}</span>
      <span className="w-32 shrink-0 text-right text-xs text-muted-foreground tabular-nums max-[899px]:hidden">{milestoneDatesLabel(milestone)}</span>
      <HealthLabel health={milestone.health} className="w-24 shrink-0 max-[640px]:hidden" />
      <MilestoneProgress milestone={milestone} />
    </li>
  )
}

function NewMilestoneModal({ projectId, onClose }: { projectId: string; onClose: () => void }) {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const createMilestone = useCreateMilestone(workspace.id, projectId)
  const [name, setName] = useState('')
  const [start, setStart] = useState('')
  const [target, setTarget] = useState('')
  const nameId = useId()
  const startId = useId()
  const targetId = useId()
  const wrongOrder = start !== '' && target !== '' && target < start

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!name.trim() || wrongOrder || createMilestone.isPending) return
    try {
      const milestone = await createMilestone.mutateAsync({ name: name.trim(), start_at: dayInputToIso(start), target_at: dayInputToIso(target) })
      onClose()
      navigate(`/tasks/projects/${projectId}/milestones/${milestone.id}`)
    } catch {
      // Keep the modal and the draft open so the user can retry.
    }
  }

  return (
    <Modal title="New milestone" onClose={onClose} className="sm:max-w-md">
      <form className="grid gap-4" onSubmit={(event) => void submit(event)}>
        <Field>
          <FieldLabel htmlFor={nameId}>Name <span className="font-semibold text-primary">*</span></FieldLabel>
          <Input id={nameId} autoFocus required maxLength={200} placeholder="v1.0" value={name} disabled={createMilestone.isPending} onChange={(event) => setName(event.target.value)} />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field>
            <FieldLabel htmlFor={startId}>Start date</FieldLabel>
            <Input id={startId} type="date" value={start} disabled={createMilestone.isPending} onChange={(event) => setStart(event.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor={targetId}>Target date</FieldLabel>
            <Input id={targetId} type="date" value={target} min={start || undefined} disabled={createMilestone.isPending} onChange={(event) => setTarget(event.target.value)} />
          </Field>
        </div>
        {wrongOrder ? <p role="alert" className="text-xs text-destructive">The target date must not be before the start date.</p> : null}
        {createMilestone.isError ? <p role="alert" className="text-xs text-destructive">The milestone was not created. Try again.</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={createMilestone.isPending} />}>Cancel</DialogClose>
          <Button type="submit" disabled={!name.trim() || wrongOrder || createMilestone.isPending}>
            {createMilestone.isPending ? 'Creating…' : 'Create milestone'}
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
