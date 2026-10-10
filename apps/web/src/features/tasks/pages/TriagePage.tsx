import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { ArrowUpRightSquare, Check, Copy, InboxIn, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { ColorDot } from '@/components/common/ColorDot'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { Button, buttonVariants } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Kbd } from '@/components/ui/kbd'
import { Textarea } from '@/components/ui/textarea'
import { useTriageQueue, useTriageTask } from '@/features/tasks/api/intake'
import { useLabels } from '@/features/tasks/api/labels'
import { taskFromRecord, type Project, type Task, type TaskStatusDef } from '@/features/tasks/api/models'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { LabelPill } from '@/features/tasks/components/TaskLabels'
import { TaskPickerDialog } from '@/features/tasks/components/TaskPickerDialog'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { projectStatuses } from '@/features/tasks/taskMeta'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { relativeTime } from '@/lib/format'
import { renderMarkdownBlocks } from '@/lib/markdown'

/** The statuses a task can be accepted into: every open status of its project, the default first. */
function acceptStatuses(statuses: TaskStatusDef[], projectId: string): TaskStatusDef[] {
  return projectStatuses(statuses, projectId).filter((status) => status.category === 'backlog' || status.category === 'unstarted' || status.category === 'started')
}

/**
 * The triage queue: tasks that came from outside the team, oldest first. A member accepts a task into the workflow,
 * declines it, or marks it as a duplicate. Keys: A accept, D decline, M duplicate, J and K to move in the list.
 */
export function TriagePage() {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id).data ?? []
  const statuses = useAllStatuses(workspace.id, projects).data
  const members = useMembers(workspace.id).data ?? []
  const labels = useLabels(workspace.id).data ?? []
  const queue = useTriageQueue(workspace.id)
  const triage = useTriageTask(workspace.id)
  const duplicates = useDuplicateActions(workspace.id)
  const [projectId, setProjectId] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [declining, setDeclining] = useState(false)
  const [pickingDuplicate, setPickingDuplicate] = useState(false)

  const projectById = new Map(projects.map((project) => [project.id, project]))
  const tasks = (queue.data?.items ?? [])
    .filter((record) => !projectId || record.project_id === projectId)
    .map((record) => taskFromRecord(record, projectById.get(record.project_id)))
  const selected = tasks.find((task) => task.id === selectedId) ?? tasks[0]
  const triageProjects = projects.filter((project) => project.triage_enabled || (queue.data?.items ?? []).some((record) => record.project_id === project.id))
  const activeProject = projectById.get(projectId ?? '')

  const act = (task: Task, body: { action: 'accept'; status_id?: string } | { action: 'decline'; comment?: string }) => {
    triage.mutate(
      { taskId: task.id, body: { expected_version: task.version, ...body } },
      {
        onSuccess: () => {
          setDeclining(false)
          toast.success(body.action === 'accept' ? `${task.identifier} accepted` : `${task.identifier} declined`)
        },
        onError: () => toast.error('The task was not changed. It changed on the server; try again.'),
      },
    )
  }

  // single keys, as in the task list; never while a field, a menu or a dialog has the keyboard
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey || event.defaultPrevented) return
      const target = event.target instanceof HTMLElement ? event.target : null
      if (target?.closest('input, textarea, select, [contenteditable="true"], [role="menu"], [role="dialog"]')) return
      if (!selected || declining || pickingDuplicate) return
      const index = tasks.findIndex((task) => task.id === selected.id)
      const key = event.key.toLowerCase()
      if (key === 'j' && tasks[index + 1]) setSelectedId(tasks[index + 1]!.id)
      else if (key === 'k' && tasks[index - 1]) setSelectedId(tasks[index - 1]!.id)
      else if (key === 'a') act(selected, { action: 'accept' })
      else if (key === 'd') setDeclining(true)
      else if (key === 'm') setPickingDuplicate(true)
      else return
      event.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <PaneTitle render={<h1 />}>Triage</PaneTitle>
          <span className="text-xs text-muted-foreground tabular-nums">{tasks.length}</span>
          {triageProjects.length > 1 ? (
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button variant="ghost" size="sm" className="ml-auto max-w-48 font-normal text-muted-foreground" aria-label="Filter by project">
                    {activeProject ? <ColorDot color={activeProject.color} /> : null}
                    <span className="truncate">{activeProject?.name ?? 'All projects'}</span>
                  </Button>
                }
              />
              <DropdownMenuContent align="end" className="w-auto min-w-45">
                <DropdownMenuItem className="data-selected:bg-accent data-selected:font-medium" data-selected={projectId === null || undefined} onClick={() => setProjectId(null)}>All projects</DropdownMenuItem>
                {triageProjects.map((project) => (
                  <DropdownMenuItem key={project.id} className="data-selected:bg-accent data-selected:font-medium" data-selected={project.id === projectId || undefined} onClick={() => setProjectId(project.id)}>
                    <ColorDot color={project.color} />
                    <span className="truncate">{project.name}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </PaneHeader>
        {queue.isPending ? null : tasks.length === 0 || !selected ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <EmptyState icon={InboxIn} title="Triage is empty" description="Tasks from GitHub and Discord show here when a project has triage on. Turn it on in the project settings." />
          </div>
        ) : (
          <div className="flex min-h-0 flex-1">
            <ul aria-label="Triage queue" className="w-[320px] shrink-0 overflow-y-auto border-r max-[899px]:w-full max-[899px]:border-r-0">
              {tasks.map((task) => (
                <li key={task.id}>
                  <button
                    type="button"
                    data-selected={task.id === selected.id || undefined}
                    aria-current={task.id === selected.id || undefined}
                    className="flex w-full flex-col gap-1 border-b px-3 py-2 text-left transition-colors outline-none hover:bg-foreground/[0.03] focus-visible:bg-foreground/[0.03] data-selected:bg-primary/10"
                    onClick={() => {
                      setSelectedId(task.id)
                      setDeclining(false)
                    }}
                  >
                    <span className="flex items-center gap-2 text-xs text-muted-foreground">
                      <ColorDot color={projectById.get(task.projectId)?.color} />
                      <span className="tabular-nums">{task.identifier}</span>
                      <span className="ml-auto">{relativeTime(task.createdAt)}</span>
                    </span>
                    <span className="truncate text-[13px] font-medium text-foreground">{task.title || 'Untitled'}</span>
                  </button>
                </li>
              ))}
            </ul>
            <TriageTask
              key={selected.id}
              task={selected}
              project={projectById.get(selected.projectId)}
              statuses={acceptStatuses(statuses, selected.projectId)}
              assignees={members.filter((member) => selected.assigneeIds.includes(member.id))}
              labels={labels.filter((label) => selected.labels.includes(label.id))}
              busy={triage.isPending}
              declining={declining}
              onAccept={(statusId) => act(selected, { action: 'accept', status_id: statusId })}
              onDecline={(comment) => act(selected, { action: 'decline', comment })}
              onDeclining={setDeclining}
              onDuplicate={() => setPickingDuplicate(true)}
            />
          </div>
        )}
      </Pane>
      {pickingDuplicate && selected ? (
        <TaskPickerDialog
          open
          onOpenChange={(open) => { if (!open) setPickingDuplicate(false) }}
          title={`Mark ${selected.identifier} as duplicate of…`}
          statuses={statuses}
          excludeIds={[selected.id]}
          excludeDuplicates
          onSelect={(target) => void duplicates.markOne(selected, target)}
        />
      ) : null}
    </div>
  )
}

function TriageTask({ task, project, statuses, assignees, labels, busy, declining, onAccept, onDecline, onDeclining, onDuplicate }: {
  task: Task
  project: Project | undefined
  statuses: TaskStatusDef[]
  assignees: User[]
  labels: Array<{ id: string; name: string; color: string }>
  busy: boolean
  declining: boolean
  onAccept: (statusId?: string) => void
  onDecline: (comment?: string) => void
  onDeclining: (open: boolean) => void
  onDuplicate: () => void
}) {
  const [comment, setComment] = useState('')
  return (
    <section aria-label={task.identifier} className="flex min-h-0 min-w-0 flex-1 flex-col max-[899px]:hidden">
      <div className="flex flex-wrap items-center gap-1.5 border-b px-4 py-2">
        <Button size="sm" disabled={busy} onClick={() => onAccept()}>
          <Check aria-hidden />
          Accept
          <Kbd className="ml-1">A</Kbd>
        </Button>
        {statuses.length > 1 ? (
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant="outline" size="sm" className="font-normal" disabled={busy}>Accept to…</Button>} />
            <DropdownMenuContent className="w-auto min-w-45">
              {statuses.map((status) => (
                <DropdownMenuItem key={status.id} onClick={() => onAccept(status.id)}>
                  <TaskStatusIcon status={status} />
                  {status.name}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
        <Button variant="outline" size="sm" disabled={busy} onClick={() => onDeclining(true)}>
          <X aria-hidden />
          Decline
          <Kbd className="ml-1">D</Kbd>
        </Button>
        <Button variant="outline" size="sm" disabled={busy} onClick={onDuplicate}>
          <Copy aria-hidden />
          Duplicate
          <Kbd className="ml-1">M</Kbd>
        </Button>
        <Link to={`/tasks/${task.id}`} className={cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'ml-auto text-muted-foreground')}>
          <ArrowUpRightSquare aria-hidden />
          Open task
        </Link>
      </div>
      {declining ? (
        <form
          className="flex flex-col gap-2 border-b bg-muted/40 px-4 py-3"
          onSubmit={(event) => {
            event.preventDefault()
            onDecline(comment.trim() || undefined)
          }}
        >
          <Textarea autoFocus aria-label="Reason" placeholder="Why is this declined? (optional)" rows={2} value={comment} disabled={busy} onChange={(event) => setComment(event.target.value)} />
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => onDeclining(false)}>Cancel</Button>
            <Button type="submit" variant="destructive" size="sm" disabled={busy}>Decline task</Button>
          </div>
        </form>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        <div className="mx-auto flex max-w-[720px] flex-col gap-3">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <ColorDot color={project?.color} />
            <span>{project?.name}</span>
            <span className="tabular-nums">{task.identifier}</span>
            {task.creatorServiceAccountName ? <span>· from {task.creatorServiceAccountName}</span> : null}
          </div>
          <h2 className="text-lg font-semibold text-foreground">{task.title || 'Untitled'}</h2>
          {labels.length > 0 || assignees.length > 0 ? (
            <div className="flex flex-wrap items-center gap-1.5">
              {labels.map((label) => <LabelPill key={label.id} label={label} />)}
              {assignees.length > 0 ? <UserAvatarStack users={assignees} size={18} /> : null}
            </div>
          ) : null}
          {task.sourceUrl ? (
            <a href={task.sourceUrl} target="_blank" rel="noopener noreferrer" className="truncate text-xs text-primary hover:underline">{task.sourceUrl}</a>
          ) : null}
          {task.description ? (
            <div className="text-[13px] leading-relaxed">{renderMarkdownBlocks(task.description, task.id)}</div>
          ) : (
            <p className="text-[13px] text-muted-foreground">No description.</p>
          )}
        </div>
      </div>
    </section>
  )
}
