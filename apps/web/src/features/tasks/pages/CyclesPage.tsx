import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { ArrowLeft, MoreH as Ellipsis, Refresh2 } from 'reicon-react'
import type { TaskQueryBody } from '@/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Tip } from '@/components/common/Tip'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { useCycleMutations, useCycles, type Cycle } from '@/features/tasks/api/cycles'
import type { Project } from '@/features/tasks/api/models'
import { useProjectStatuses, useProjects } from '@/features/tasks/api/projects'
import { TaskProgress } from '@/features/tasks/components/MilestoneBits'
import { EstimateChip } from '@/features/tasks/components/TaskPropertyChips'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { STATE_LABEL, cycleDatesLabel, cycleLastDay, cycleName, dayInput, moveBoundary, pointsText } from '@/features/tasks/cyclesLib'
import { BurndownChart, VelocityChart } from '@/features/tasks/insights/InsightCharts'
import { taskFromRecord } from '@/features/tasks/api/models'
import { useCreateTaskDefaults, useOpenNewTask } from '@/features/tasks/newTask'
import { taskQueryOptions } from '@/features/views/api/taskQuery'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** "5 of 12 points · 3 of 8 tasks": the scope and what is done, in points where the scope has points. */
function scopeText(cycle: Cycle): string {
  const tasks = `${cycle.done_count} of ${cycle.scope_count} ${cycle.scope_count === 1 ? 'task' : 'tasks'}`
  return cycle.scope_points > 0 ? `${cycle.done_points} of ${pointsText(cycle.scope_points)} · ${tasks}` : tasks
}

/** Every cycle of a project: the current one, the upcoming ones, and the completed ones (newest first). */
export function CyclesPage() {
  const { projectId } = useParams()
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const project = useProjects(workspace.id).data?.find((item) => item.id === projectId)
  const cyclesQuery = useCycles(workspace.id, projectId)
  const cycles = cyclesQuery.data ?? []
  const open = cycles.filter((cycle) => cycle.state !== 'completed')
  const completed = cycles.filter((cycle) => cycle.state === 'completed').reverse()
  const back = `/tasks/projects/${projectId}`
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <Tip label={project?.name ?? 'Project'} side="bottom">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate(back)} aria-label="Back to the project">
              <ArrowLeft className="size-4" />
            </Button>
          </Tip>
          {project ? <Link to={back} className="truncate text-[13px] text-muted-foreground outline-none hover:underline focus-visible:underline">{project.name}</Link> : null}
          <span aria-hidden className="text-muted-foreground/50">/</span>
          <PaneTitle render={<h1 />}>Cycles</PaneTitle>
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {cyclesQuery.isPending ? null : cycles.length === 0 ? (
            <EmptyState icon={Refresh2} title="No cycles" description="Turn cycles on in the project settings. Orbit then creates each cycle." />
          ) : (
            <div className="mx-auto flex w-full max-w-[800px] flex-col gap-6 px-10 pt-7 pb-12 max-[899px]:px-5">
              {project ? <VelocityChart cycles={cycles} project={project} /> : null}
              {[{ title: 'Current and upcoming', items: open }, { title: 'Completed', items: completed }].map((section) => section.items.length > 0 ? (
                <section key={section.title} aria-label={section.title} className="flex flex-col gap-2">
                  <h2 className="text-xs font-medium text-muted-foreground/70">{section.title}</h2>
                  <ul className="rounded-lg border">
                    {section.items.map((cycle) => (
                      <li key={cycle.id} className="flex min-h-11 items-center gap-3 border-b px-3 py-1.5 text-[13px] last:border-b-0">
                        <span className="w-8 shrink-0 text-xs text-muted-foreground tabular-nums">#{cycle.number}</span>
                        <Link to={`${back}/cycles/${cycle.id}`} className="min-w-0 flex-1 truncate font-medium outline-none hover:underline focus-visible:underline">{cycleName(cycle)}</Link>
                        {cycle.state === 'current' ? <span className="shrink-0 rounded-full bg-primary/10 px-2 py-0.5 text-[11px] font-medium text-primary">Current</span> : null}
                        <span className="w-36 shrink-0 text-right text-xs text-muted-foreground tabular-nums max-[640px]:hidden">{cycleDatesLabel(cycle)}</span>
                        <TaskProgress done={cycle.scope_points > 0 ? cycle.done_points : cycle.done_count} total={cycle.scope_points > 0 ? cycle.scope_points : cycle.scope_count} />
                      </li>
                    ))}
                  </ul>
                </section>
              ) : null)}
            </div>
          )}
        </div>
      </Pane>
    </div>
  )
}

/** One cycle: its dates, its scope and progress, the manual actions, and its tasks. */
export function CyclePage() {
  const { projectId, cycleId } = useParams()
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const project = useProjects(workspace.id).data?.find((item) => item.id === projectId)
  const cyclesQuery = useCycles(workspace.id, projectId)
  const cycles = cyclesQuery.data ?? []
  const cycle = cycles.find((item) => item.id === cycleId)
  const back = `/tasks/projects/${projectId}/cycles`
  // "Start today" is an action of the next cycle only
  const next = cycles.find((item) => item.state === 'future')
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <Tip label="All cycles" side="bottom">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate(back)} aria-label="All cycles">
              <ArrowLeft className="size-4" />
            </Button>
          </Tip>
          {project ? <Link to={`/tasks/projects/${project.id}`} className="truncate text-[13px] text-muted-foreground outline-none hover:underline focus-visible:underline">{project.name}</Link> : null}
          <span aria-hidden className="text-muted-foreground/50">/</span>
          <PaneTitle render={<h1 />}>{cycle ? cycleName(cycle) : 'Cycle'}</PaneTitle>
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {cyclesQuery.isPending ? null : !cycle || !project ? (
            <EmptyState icon={Refresh2} title="Cycle not found" description="This cycle does not exist or was deleted with a settings change." />
          ) : (
            <CycleBody key={`${cycle.id}:${cycle.version}`} cycle={cycle} project={project} isNext={next?.id === cycle.id} />
          )}
        </div>
      </Pane>
    </div>
  )
}

function CycleBody({ cycle, project, isNext }: { cycle: Cycle; project: Project; isNext: boolean }) {
  const { workspace } = useWorkspace()
  const { update, startToday, endToday } = useCycleMutations(workspace.id, project.id)
  const [name, setName] = useState(cycle.name ?? '')
  const openNewTask = useOpenNewTask()
  const completed = cycle.state === 'completed'
  const firstDay = dayInput(new Date(cycle.starts_at))
  const lastDay = dayInput(cycleLastDay(cycle))

  // "New task" (and the C key) on this page makes a task of this cycle
  useCreateTaskDefaults({ defaults: () => (completed ? { project_id: project.id } : { project_id: project.id, cycle_id: cycle.id }), onOpenTask: () => {} })

  const startNow = async () => {
    if (await confirmAction({ title: `Start ${cycleName(cycle)} today?`, description: 'The current cycle ends now, and its open tasks move to this cycle. The later cycles move with it.', confirmLabel: 'Start today' })) startToday.mutate(cycle.id)
  }
  const endNow = async () => {
    if (await confirmAction({ title: `End ${cycleName(cycle)} today?`, description: 'The cycle ends at the end of today. Its open tasks then move to the next cycle.', confirmLabel: 'End today' })) endToday.mutate(cycle.id)
  }
  const failed = update.isError || startToday.isError || endToday.isError
  return (
    <div className="mx-auto flex w-full max-w-[800px] flex-col gap-8 px-10 pt-7 pb-12 max-[899px]:px-5">
      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <Input
            aria-label="Cycle name"
            className="h-9 flex-1 border-transparent px-1 text-lg font-semibold shadow-none hover:border-input focus-visible:border-input md:text-lg dark:bg-transparent"
            maxLength={200}
            placeholder={`Cycle ${cycle.number}`}
            value={name}
            disabled={completed}
            onChange={(event) => setName(event.target.value)}
            onBlur={() => {
              const next = name.trim()
              if (next !== (cycle.name ?? '')) update.mutate({ cycleId: cycle.id, body: { expected_version: cycle.version, name: next || null } })
            }}
            onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur() }}
          />
          {cycle.state === 'current' || isNext ? (
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Cycle actions"><Ellipsis /></Button>} />
              <DropdownMenuContent align="end" className="w-auto min-w-44">
                {isNext ? <DropdownMenuItem onClick={() => void startNow()}>Start this cycle today…</DropdownMenuItem> : null}
                {cycle.state === 'current' ? <DropdownMenuItem onClick={() => void endNow()}>End this cycle today…</DropdownMenuItem> : null}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </div>
        <dl className="flex flex-wrap items-center gap-x-6 gap-y-2 text-[13px]">
          <div className="flex items-center gap-2"><dt className="text-xs text-muted-foreground/70">State</dt><dd>{STATE_LABEL[cycle.state] ?? cycle.state}</dd></div>
          {cycle.state === 'future' ? (
            // only a future cycle can move; the later future cycles move by the same amount as its end
            <div className="flex items-center gap-2">
              <dt className="text-xs text-muted-foreground/70">Dates</dt>
              <dd className="flex items-center gap-1.5">
                <Input
                  type="date"
                  aria-label="First day"
                  className="h-8 w-36 text-[13px]"
                  value={firstDay}
                  max={lastDay}
                  disabled={update.isPending}
                  onChange={(event) => {
                    if (event.target.value && event.target.value !== firstDay) update.mutate({ cycleId: cycle.id, body: { expected_version: cycle.version, starts_at: moveBoundary(cycle.starts_at, firstDay, event.target.value) } })
                  }}
                />
                <span aria-hidden className="text-muted-foreground">–</span>
                <Input
                  type="date"
                  aria-label="Last day"
                  className="h-8 w-36 text-[13px]"
                  value={lastDay}
                  min={firstDay}
                  disabled={update.isPending}
                  onChange={(event) => {
                    if (event.target.value && event.target.value !== lastDay) update.mutate({ cycleId: cycle.id, body: { expected_version: cycle.version, ends_at: moveBoundary(cycle.ends_at, lastDay, event.target.value) } })
                  }}
                />
              </dd>
            </div>
          ) : (
            <div className="flex items-center gap-2"><dt className="text-xs text-muted-foreground/70">Dates</dt><dd className="tabular-nums">{cycleDatesLabel(cycle)}</dd></div>
          )}
          <div className="flex items-center gap-2"><dt className="text-xs text-muted-foreground/70">Scope</dt><dd className="tabular-nums">{cycle.scope_points > 0 ? `${pointsText(cycle.scope_points)} · ` : ''}{cycle.scope_count} {cycle.scope_count === 1 ? 'task' : 'tasks'}</dd></div>
          <div className="flex items-center gap-2"><dt className="text-xs text-muted-foreground/70">Started</dt><dd className="tabular-nums">{cycle.scope_points > 0 ? `${cycle.started_points} · ` : ''}{cycle.started_count}</dd></div>
          <div className="flex items-center gap-2"><dt className="text-xs text-muted-foreground/70">Done</dt><dd className="tabular-nums">{scopeText(cycle)}</dd></div>
          {project.estimate_scale && cycle.unestimated_count > 0 ? (
            <div className="flex items-center gap-2"><dt className="text-xs text-muted-foreground/70">No estimate</dt><dd className="tabular-nums">{cycle.unestimated_count}</dd></div>
          ) : null}
        </dl>
        {failed ? <p role="alert" className="text-xs text-destructive">The change was not saved. A cycle must not start before the cycle before it ends; or the cycle changed on the server.</p> : null}
      </section>
      {cycle.state === 'future' ? null : <BurndownChart cycle={cycle} project={project} />}
      <section aria-label="Tasks" className="flex flex-col gap-2">
        <div className="flex items-center">
          <h2 className="text-xs font-medium text-muted-foreground/70">Tasks</h2>
          {completed ? null : <Button variant="ghost" size="sm" className="ml-auto" onClick={() => openNewTask({})}>New task</Button>}
        </div>
        <CycleTasks cycle={cycle} project={project} />
      </section>
    </div>
  )
}

/** The tasks of the cycle in manual order, each a link to the task. */
function CycleTasks({ cycle, project }: { cycle: Cycle; project: Project }) {
  const { workspace } = useWorkspace()
  const statuses = useProjectStatuses(workspace.id, project.id).data ?? []
  const members = useMembers(workspace.id).data ?? []
  const body: TaskQueryBody = {
    filter: { op: 'and', children: [{ field: 'cycle', operator: 'is', value: [cycle.id] }] } as unknown as TaskQueryBody['filter'],
    order_by: 'manual',
    order_direction: 'asc',
    show_completed: 'all',
  }
  const records = useQuery(taskQueryOptions(workspace.id, body)).data?.items ?? []
  if (records.length === 0) return <p className="text-[13px] text-muted-foreground">No task is in this cycle.</p>
  return (
    <ul className="rounded-lg border">
      {records.map((record) => {
        const task = taskFromRecord(record, project)
        return (
          <li key={task.id} className="border-b last:border-b-0">
            <Link to={`/tasks/${task.id}`} className="flex min-h-9 items-center gap-2.5 px-3 py-1.5 text-[13px] transition-colors outline-none hover:bg-foreground/[0.03] focus-visible:bg-foreground/[0.03]">
              <TaskStatusIcon status={statuses.find((status) => status.id === task.statusId)} />
              <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{task.identifier}</span>
              <span className="min-w-0 flex-1 truncate">{task.title}</span>
              <EstimateChip task={task} project={project} className="text-[11px]" />
              {task.assigneeIds.length > 0 ? <UserAvatarStack users={members.filter((member) => task.assigneeIds.includes(member.id))} size={18} /> : null}
            </Link>
          </li>
        )
      })}
    </ul>
  )
}
