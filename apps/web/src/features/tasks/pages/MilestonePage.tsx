import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { Add as Plus, ArrowLeft, MoreH as Ellipsis, Signpost } from 'reicon-react'
import type { TaskQueryBody } from '@/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { FavoriteStar } from '@/features/favorites/FavoriteStar'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Tip } from '@/components/common/Tip'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  HEALTH_LABEL, HEALTH_VALUES, MILESTONE_STATUSES, MILESTONE_STATUS_LABEL,
  useDeleteMilestone, useEnsureMilestoneDescriptionPage, useMilestoneUpdateMutations, useMilestoneUpdates, useMilestones, useUpdateMilestone,
  type Milestone, type MilestoneHealth, type MilestoneUpdate,
} from '@/features/tasks/api/milestones'
import type { Project } from '@/features/tasks/api/models'
import { useProjectStatuses, useProjects } from '@/features/tasks/api/projects'
import { DescriptionEditor } from '@/features/tasks/components/DescriptionEditor'
import { HealthDot, HealthLabel, MilestoneProgress } from '@/features/tasks/components/MilestoneBits'
import { BurnupChart } from '@/features/tasks/insights/InsightCharts'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useCreateTaskDefaults, useOpenNewTask } from '@/features/tasks/newTask'
import { dayInputToIso, isHealth, isMilestoneStatus, isoToDayInput } from '@/features/tasks/roadmap/roadmapLib'
import { taskQueryOptions } from '@/features/views/api/taskQuery'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { relativeTime } from '@/lib/format'
import { renderMarkdownBlocks } from '@/lib/markdown'

/** One milestone: status, dates, description, health updates and its tasks. */
export function MilestonePage() {
  const { projectId, milestoneId } = useParams()
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const project = useProjects(workspace.id).data?.find((item) => item.id === projectId)
  const milestonesQuery = useMilestones(workspace.id)
  const milestone = milestonesQuery.data?.find((item) => item.id === milestoneId && item.project_id === projectId)
  const back = `/tasks/projects/${projectId}`

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <Tip label={project ? project.name : 'Project'} side="bottom">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate(back)} aria-label="Back to the project">
              <ArrowLeft className="size-4" />
            </Button>
          </Tip>
          {project ? <Link to={back} className="truncate text-[13px] text-muted-foreground outline-none hover:underline focus-visible:underline">{project.name}</Link> : null}
          <span aria-hidden className="text-muted-foreground/50">/</span>
          <PaneTitle render={<h1 />}>{milestone?.name ?? 'Milestone'}</PaneTitle>
          {milestone ? <FavoriteStar workspaceId={workspace.id} kind="milestone" targetId={milestone.id} /> : null}
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {milestonesQuery.isPending ? null : !milestone || !project ? (
            <EmptyState icon={Signpost} title="Milestone not found" description="This milestone does not exist or was deleted." />
          ) : (
            <MilestoneBody key={milestone.id} milestone={milestone} project={project} onDeleted={() => navigate(back, { replace: true })} />
          )}
        </div>
      </Pane>
    </div>
  )
}

function MilestoneBody({ milestone, project, onDeleted }: { milestone: Milestone; project: Project; onDeleted: () => void }) {
  const { workspace } = useWorkspace()
  const members = useMembers(workspace.id).data ?? []
  const updateMilestone = useUpdateMilestone(workspace.id)
  const deleteMilestone = useDeleteMilestone(workspace.id)
  const ensurePage = useEnsureMilestoneDescriptionPage(workspace.id)
  const [name, setName] = useState(milestone.name)
  const openNewTask = useOpenNewTask()

  // "New task" (and the C key) on this page makes a task of this milestone
  useCreateTaskDefaults({ defaults: () => ({ project_id: project.id, milestone_id: milestone.id }), onOpenTask: () => {} })

  const save = (body: { name?: string; status?: string; start_at?: string | null; target_at?: string | null }) =>
    updateMilestone.mutate({ milestone, body: { expected_version: milestone.version, ...body } })

  const remove = async () => {
    const confirmed = await confirmAction({
      title: `Delete “${milestone.name}”?`,
      description: 'The description and the updates are deleted. The tasks stay in the project with no milestone. You cannot undo this.',
      confirmLabel: 'Delete milestone',
      danger: true,
    })
    if (!confirmed) return
    deleteMilestone.mutate(milestone, { onSuccess: onDeleted })
  }

  return (
    <div className="mx-auto flex w-full max-w-[800px] flex-col gap-8 px-10 pt-7 pb-12 max-[899px]:px-5">
      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <Input
            aria-label="Milestone name"
            className="h-9 flex-1 border-transparent px-1 text-lg font-semibold shadow-none hover:border-input focus-visible:border-input md:text-lg dark:bg-transparent"
            maxLength={200}
            value={name}
            onChange={(event) => setName(event.target.value)}
            onBlur={() => {
              const next = name.trim()
              if (!next) setName(milestone.name)
              else if (next !== milestone.name) save({ name: next })
            }}
            onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur() }}
          />
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Milestone actions"><Ellipsis /></Button>} />
            <DropdownMenuContent align="end" className="w-auto min-w-40">
              <DropdownMenuItem variant="destructive" onClick={() => void remove()}>Delete milestone…</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[13px]">
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button variant="outline" size="sm" className="font-normal" aria-label={`Status: ${milestone.status}`}>{isMilestoneStatus(milestone.status) ? MILESTONE_STATUS_LABEL[milestone.status] : milestone.status}</Button>}
            />
            <DropdownMenuContent className="w-auto min-w-40">
              {MILESTONE_STATUSES.map((status) => (
                <DropdownMenuItem key={status} className="data-selected:bg-accent data-selected:font-medium" data-selected={status === milestone.status || undefined} onClick={() => save({ status })}>
                  {MILESTONE_STATUS_LABEL[status]}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <label className="flex items-center gap-2 text-xs text-muted-foreground/70">
            Start
            <Input
              type="date"
              className="h-8 w-36 text-[13px] text-foreground"
              value={isoToDayInput(milestone.start_at)}
              max={isoToDayInput(milestone.target_at) || undefined}
              onChange={(event) => save({ start_at: dayInputToIso(event.target.value) })}
            />
          </label>
          <label className="flex items-center gap-2 text-xs text-muted-foreground/70">
            Target
            <Input
              type="date"
              className="h-8 w-36 text-[13px] text-foreground"
              value={isoToDayInput(milestone.target_at)}
              min={isoToDayInput(milestone.start_at) || undefined}
              onChange={(event) => save({ target_at: dayInputToIso(event.target.value) })}
            />
          </label>
          <HealthLabel health={milestone.health} />
          <MilestoneProgress milestone={milestone} />
        </div>
        {updateMilestone.isError ? <p role="alert" className="text-xs text-destructive">The change was not saved. The target date must not be before the start date.</p> : null}
      </section>

      <section aria-label="Description" className="flex flex-col gap-2">
        <h2 className="text-xs font-medium text-muted-foreground/70">Description</h2>
        <DescriptionEditor workspaceId={workspace.id} pageId={milestone.description_page_id} onCreate={() => ensurePage.mutateAsync(milestone)} />
      </section>

      <BurnupChart milestone={milestone} project={project} />

      <UpdateFeed milestone={milestone} members={members} />

      <section aria-label="Tasks" className="flex flex-col gap-2">
        <div className="flex items-center">
          <h2 className="text-xs font-medium text-muted-foreground/70">Tasks</h2>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={() => openNewTask({})}>
            <Plus aria-hidden />
            New task
          </Button>
        </div>
        <MilestoneTasks milestone={milestone} project={project} members={members} />
      </section>
    </div>
  )
}

function UpdateFeed({ milestone, members }: { milestone: Milestone; members: User[] }) {
  const { workspace } = useWorkspace()
  const updates = useMilestoneUpdates(workspace.id, milestone).data ?? []
  const { post, edit, remove } = useMilestoneUpdateMutations(workspace.id, milestone)
  const [editingId, setEditingId] = useState<string | null>(null)

  return (
    <section aria-label="Updates" className="flex flex-col gap-3">
      <h2 className="text-xs font-medium text-muted-foreground/70">Updates</h2>
      <UpdateComposer
        key={updates.length}
        initialHealth={isHealth(milestone.health) ? milestone.health : 'on_track'}
        submitLabel="Post update"
        busy={post.isPending}
        failed={post.isError}
        onSubmit={(health, body) => post.mutate({ health, body })}
      />
      {updates.length > 0 ? (
        <ul className="flex flex-col gap-3">
          {updates.map((update) => {
            const author = members.find((member) => member.id === update.author_id)
            return (
              <li key={update.id} className="rounded-lg border px-3 py-2.5">
                {editingId === update.id ? (
                  <UpdateComposer
                    initialHealth={isHealth(update.health) ? update.health : 'on_track'}
                    initialBody={update.body}
                    submitLabel="Save"
                    busy={edit.isPending}
                    failed={edit.isError}
                    onCancel={() => setEditingId(null)}
                    onSubmit={(health, body) => edit.mutate({ updateId: update.id, body: { expected_version: update.version, health, body } }, { onSuccess: () => setEditingId(null) })}
                  />
                ) : (
                  <UpdateItem update={update} author={author} onEdit={() => setEditingId(update.id)} onDelete={() => remove.mutate(update)} />
                )}
              </li>
            )
          })}
        </ul>
      ) : null}
    </section>
  )
}

function UpdateItem({ update, author, onEdit, onDelete }: { update: MilestoneUpdate; author: User | undefined; onEdit: () => void; onDelete: () => void }) {
  const confirmDelete = async () => {
    if (await confirmAction({ title: 'Delete this update?', description: 'The health of the milestone becomes the health of the update before it.', confirmLabel: 'Delete update', danger: true })) onDelete()
  }
  return (
    <>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <UserAvatar user={author} size={18} name={author ? undefined : '?'} />
        <span className="font-medium text-foreground">{author?.name ?? 'Former member'}</span>
        <HealthLabel health={update.health} />
        <span title={update.created_at}>{relativeTime(update.created_at)}{update.updated_at !== update.created_at ? ' · edited' : ''}</span>
        {update.can_edit || update.can_delete ? (
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant="ghost" size="icon-xs" className="ml-auto text-muted-foreground/70" aria-label="Update actions"><Ellipsis /></Button>} />
            <DropdownMenuContent align="end" className="w-auto min-w-32">
              {update.can_edit ? <DropdownMenuItem onClick={onEdit}>Edit</DropdownMenuItem> : null}
              {update.can_delete ? <DropdownMenuItem variant="destructive" onClick={() => void confirmDelete()}>Delete…</DropdownMenuItem> : null}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
      </div>
      <div className="mt-2 text-[13px] leading-relaxed">{renderMarkdownBlocks(update.body, update.id)}</div>
    </>
  )
}

function UpdateComposer({ initialHealth, initialBody = '', submitLabel, busy, failed, onSubmit, onCancel }: {
  initialHealth: MilestoneHealth
  initialBody?: string
  submitLabel: string
  busy: boolean
  failed: boolean
  onSubmit: (health: MilestoneHealth, body: string) => void
  onCancel?: () => void
}) {
  const [health, setHealth] = useState(initialHealth)
  const [body, setBody] = useState(initialBody)
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (body.trim() && !busy) onSubmit(health, body.trim())
      }}
    >
      <Textarea aria-label="Update" placeholder="How is this milestone going? Markdown is supported." rows={3} maxLength={20000} value={body} disabled={busy} onChange={(event) => setBody(event.target.value)} />
      <div className="flex items-center gap-2">
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="outline" size="sm" className="font-normal" aria-label={`Health: ${HEALTH_LABEL[health]}`}>
                <HealthDot health={health} />
                {HEALTH_LABEL[health]}
              </Button>
            }
          />
          <DropdownMenuContent className="w-auto min-w-36">
            {HEALTH_VALUES.map((value) => (
              <DropdownMenuItem key={value} onClick={() => setHealth(value)}>
                <HealthDot health={value} />
                {HEALTH_LABEL[value]}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        {failed ? <span role="alert" className="text-xs text-destructive">Not saved. Try again.</span> : null}
        <div className="ml-auto flex items-center gap-2">
          {onCancel ? <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={onCancel}>Cancel</Button> : null}
          <Button type="submit" size="sm" disabled={!body.trim() || busy}>{submitLabel}</Button>
        </div>
      </div>
    </form>
  )
}

/** The tasks of the milestone, newest first, each a link to the task. */
function MilestoneTasks({ milestone, project, members }: { milestone: Milestone; project: Project; members: User[] }) {
  const { workspace } = useWorkspace()
  const statuses = useProjectStatuses(workspace.id, project.id).data ?? []
  const body: TaskQueryBody = {
    filter: { op: 'and', children: [{ field: 'milestone', operator: 'is', value: [milestone.id] }] } as unknown as TaskQueryBody['filter'],
    order_by: 'created',
    order_direction: 'desc',
    show_completed: 'all',
  }
  const tasks = useQuery(taskQueryOptions(workspace.id, body)).data?.items ?? []

  if (tasks.length === 0) return <p className="text-[13px] text-muted-foreground">No task belongs to this milestone.</p>
  return (
    <ul className="rounded-lg border">
      {tasks.map((task) => (
        <li key={task.id} className="border-b last:border-b-0">
          <Link to={`/tasks/${task.id}`} className="flex min-h-9 items-center gap-2.5 px-3 py-1.5 text-[13px] transition-colors outline-none hover:bg-foreground/[0.03] focus-visible:bg-foreground/[0.03]">
            <TaskStatusIcon status={statuses.find((status) => status.id === task.status_id)} />
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{project.key}-{task.number}</span>
            <span className="min-w-0 flex-1 truncate">{task.title}</span>
            {task.assignee_ids.length > 0 ? <UserAvatarStack users={members.filter((member) => task.assignee_ids.includes(member.id))} size={18} /> : null}
          </Link>
        </li>
      ))}
    </ul>
  )
}
