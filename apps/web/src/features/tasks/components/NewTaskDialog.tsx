import { useRef, useState } from 'react'
import { Calendar, ChevronRight } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogTitle } from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import type { CreateTaskBody, TaskRecord } from '@/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { DatePicker } from '@/components/common/DatePicker'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useLabels } from '@/features/tasks/api/labels'
import { taskIdentifier, type TaskPriority } from '@/features/tasks/api/models'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { useCreateTask } from '@/features/tasks/api/tasks'
import { dueDateLabel, PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses, statusKeyOf } from '@/features/tasks/taskMeta'
import { resolveStatusId } from '@/features/tasks/tasksLib'
import { PriorityIcon } from './PriorityIcon'
import { TaskLabels } from './TaskLabels'
import { TaskStatusIcon } from './TaskStatusIcon'

const CREATE_MORE_KEY = 'orbit:new_task_create_more'

interface NewTaskDialogProps {
  /** Starting properties (the page's filter, a group header). Unknown or missing ones fall back to the first project and its default status. */
  defaults?: Partial<CreateTaskBody>
  /** Opened from the keyboard: no entrance animation. */
  instant?: boolean
  /** Called after the exit has finished. */
  onClose: () => void
  /** Opens a created task: at once without "Create more", else from the "Created" notice. */
  onOpenTask: (task: TaskRecord) => void
}

/** Creates tasks without leaving the page. Callers mount it while open. With "Create more" on, the
    dialog stays open after each task and keeps the properties, so a batch needs only titles. */
export function NewTaskDialog({ defaults = {}, instant, onClose, onOpenTask }: NewTaskDialogProps) {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id).data ?? []
  const statuses = useAllStatuses(workspace.id, projects).data
  const members = useMembers(workspace.id).data ?? []
  const labels = useLabels(workspace.id).data ?? []
  const createTask = useCreateTask(workspace.id)
  const [open, setOpen] = useState(true)
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [draft, setDraft] = useState(() => ({
    projectId: defaults.project_id,
    statusId: defaults.status_id,
    priority: (defaults.priority ?? 'none') as TaskPriority,
    assigneeIds: defaults.assignee_ids ?? [],
    labelIds: defaults.label_ids ?? [],
    dueAt: defaults.due_at ?? null,
    dueStartAt: defaults.due_start_at ?? null,
  }))
  const [createMore, setCreateMore] = useState(() => window.localStorage.getItem(CREATE_MORE_KEY) === '1')
  const [created, setCreated] = useState<TaskRecord>()
  const [dueDateOpen, setDueDateOpen] = useState(false)
  const titleRef = useRef<HTMLInputElement>(null)
  const descriptionRef = useRef<HTMLTextAreaElement>(null)

  // projects and statuses can still load when the dialog opens outside Tasks, so the fallbacks are derived
  const project = projects.find((candidate) => candidate.id === draft.projectId) ?? projects[0]
  // a Duplicate status needs a canonical task, which a new task cannot have yet
  const statusOptions = project ? projectStatuses(statuses, project.id).filter((option) => option.category !== 'duplicate') : []
  const status = statusOptions.find((option) => option.id === draft.statusId)
    ?? statusOptions.find((option) => option.id === (project ? resolveStatusId(statuses, project.id, null) : undefined))
    ?? statusOptions[0]
  const assignees = members.filter((member) => draft.assigneeIds.includes(member.id))
  const canCreate = title.trim() !== '' && project !== undefined && status !== undefined && !createTask.isPending

  const openTask = (task: TaskRecord) => {
    setOpen(false)
    onOpenTask(task)
  }
  const submit = async () => {
    if (!canCreate) return
    try {
      const task = await createTask.mutateAsync({
        title: title.trim(),
        description: description.trim(),
        project_id: project.id,
        status_id: status.id,
        priority: draft.priority,
        assignee_ids: draft.assigneeIds,
        label_ids: draft.labelIds,
        due_at: draft.dueAt,
        due_start_at: draft.dueStartAt,
      })
      if (!createMore) return openTask(task)
      // the properties stay for the next task of the batch
      setCreated(task)
      setTitle('')
      setDescription('')
      titleRef.current?.focus()
    } catch {
      // The draft stays; the mutation shows the error beside the create action.
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next, details) => {
        if (next) return
        // a request in flight must finish here, and a stray click beside the dialog must not drop a draft
        if (createTask.isPending || (details.reason === 'outside-press' && (title.trim() || description.trim()))) details.cancel()
        else setOpen(false)
      }}
      onOpenChangeComplete={(next) => {
        if (!next) onClose()
      }}
    >
      {/* anchored to the top so the dialog grows downward as the description grows */}
      <DialogContent instant={instant} initialFocus={titleRef} className="top-[14vh] max-h-[80vh] translate-y-0 gap-3 sm:max-w-2xl">
        <form
          className="contents"
          onSubmit={(event) => {
            event.preventDefault()
            void submit()
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
              event.preventDefault()
              void submit()
            }
          }}
        >
          <div className="flex items-center gap-1.5 pr-8">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button type="button" variant="outline" size="xs" className="max-w-48 font-normal" aria-label={`Project: ${project?.name ?? 'none'}`}>
                    <ColorDot color={project?.color} />
                    <span className="truncate">{project?.name ?? 'No project'}</span>
                  </Button>
                }
              />
              <DropdownMenuContent className="w-auto min-w-45">
                {projects.map((option) => (
                  <DropdownMenuItem
                    key={option.id}
                    className="data-selected:bg-accent data-selected:font-medium"
                    data-selected={option.id === project?.id || undefined}
                    // the status follows by name into the other project's workflow
                    onClick={() => setDraft((current) => ({ ...current, projectId: option.id, statusId: resolveStatusId(statuses, option.id, status ? statusKeyOf(status) : null) }))}
                  >
                    <ColorDot color={option.color} />
                    <span className="truncate">{option.name}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <ChevronRight aria-hidden className="size-3 text-muted-foreground/70" />
            <DialogTitle className="text-xs font-normal text-muted-foreground">New task</DialogTitle>
          </div>

          <div className="flex min-h-0 flex-col gap-1.5 overflow-y-auto">
            <Input
              ref={titleRef}
              className="h-auto rounded-none border-0 bg-transparent p-0 text-lg font-medium focus-visible:ring-0 md:text-lg dark:bg-transparent"
              value={title}
              placeholder="Task title"
              aria-label="Task title"
              readOnly={createTask.isPending}
              onChange={(event) => setTitle(event.target.value)}
              onKeyDown={(event) => {
                // Enter moves on to the description; Cmd/Ctrl+Enter creates (handled by the form)
                if (event.key === 'Enter' && !event.metaKey && !event.ctrlKey) {
                  event.preventDefault()
                  descriptionRef.current?.focus()
                }
              }}
            />
            <Textarea
              ref={descriptionRef}
              className="block min-h-20 resize-none rounded-none border-0 bg-transparent p-0 text-[13px] leading-5 focus-visible:ring-0 md:text-[13px] dark:bg-transparent"
              value={description}
              placeholder="Add a description…"
              aria-label="Description"
              readOnly={createTask.isPending}
              onChange={(event) => setDescription(event.target.value)}
            />
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button type="button" variant="outline" size="sm" className="font-normal" aria-label={`Status: ${status?.name ?? 'none'}`}>
                    <TaskStatusIcon status={status} />
                    {status?.name ?? 'No status'}
                  </Button>
                }
              />
              <DropdownMenuContent className="w-auto min-w-45">
                {statusOptions.map((option) => (
                  <DropdownMenuItem
                    key={option.id}
                    className="data-selected:bg-accent data-selected:font-medium"
                    data-selected={option.id === status?.id || undefined}
                    onClick={() => setDraft((current) => ({ ...current, statusId: option.id }))}
                  >
                    <TaskStatusIcon status={option} />
                    {option.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button type="button" variant="outline" size="sm" className="font-normal" aria-label={`Priority: ${PRIORITY_LABEL[draft.priority]}`}>
                    <PriorityIcon priority={draft.priority} />
                    {draft.priority === 'none' ? 'Priority' : PRIORITY_LABEL[draft.priority]}
                  </Button>
                }
              />
              <DropdownMenuContent className="w-auto min-w-45">
                {PRIORITY_ORDER.map((priority) => (
                  <DropdownMenuItem
                    key={priority}
                    className="data-selected:bg-accent data-selected:font-medium"
                    data-selected={priority === draft.priority || undefined}
                    onClick={() => setDraft((current) => ({ ...current, priority }))}
                  >
                    <PriorityIcon priority={priority} />
                    {PRIORITY_LABEL[priority]}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button type="button" variant="outline" size="sm" className="max-w-56 font-normal" aria-label="Assignees">
                    {assignees.length > 0 ? (
                      <>
                        <UserAvatarStack users={assignees} size={16} />
                        <span className="truncate">{assignees.map((member) => member.name).join(', ')}</span>
                      </>
                    ) : (
                      <>
                        <UserAvatar user={undefined} size={16} name="—" />
                        Assignee
                      </>
                    )}
                  </Button>
                }
              />
              <DropdownMenuContent className="w-auto min-w-45">
                {members.map((member) => {
                  const active = draft.assigneeIds.includes(member.id)
                  return (
                    <DropdownMenuCheckboxItem
                      key={member.id}
                      checked={active}
                      onCheckedChange={() => setDraft((current) => ({ ...current, assigneeIds: active ? current.assigneeIds.filter((id) => id !== member.id) : [...current.assigneeIds, member.id] }))}
                    >
                      <UserAvatar user={member} size={16} />
                      {member.name}
                    </DropdownMenuCheckboxItem>
                  )
                })}
              </DropdownMenuContent>
            </DropdownMenu>
            <Popover open={dueDateOpen} onOpenChange={setDueDateOpen}>
              <PopoverTrigger
                render={
                  <Button type="button" variant="outline" size="sm" className="font-normal" aria-label="Due date">
                    <Calendar aria-hidden="true" />
                    {draft.dueAt ? dueDateLabel(draft.dueAt, draft.dueStartAt) : 'Due date'}
                  </Button>
                }
              />
              <PopoverContent align="start" className="w-auto gap-0 p-0">
                <DatePicker
                  startValue={draft.dueStartAt}
                  value={draft.dueAt}
                  onClear={() => {
                    setDraft((current) => ({ ...current, dueAt: null, dueStartAt: null }))
                    setDueDateOpen(false)
                  }}
                  onDone={({ start, end }) => {
                    setDraft((current) => ({ ...current, dueAt: end, dueStartAt: start }))
                    setDueDateOpen(false)
                  }}
                />
              </PopoverContent>
            </Popover>
            <TaskLabels workspaceId={workspace.id} labelIds={draft.labelIds} labels={labels} onChange={(labelIds) => setDraft((current) => ({ ...current, labelIds }))} />
          </div>

          <DialogFooter className="items-center py-2.5 sm:justify-between">
            <div className="min-w-0 text-xs">
              {createTask.isError ? (
                <span role="alert" className="text-destructive">The task was not created. Try again.</span>
              ) : created ? (
                <span role="status" className="flex items-center gap-1 text-muted-foreground">
                  Created
                  <Button type="button" variant="link" size="xs" className="px-0" onClick={() => openTask(created)}>
                    {taskIdentifier(created.id, projects.find((candidate) => candidate.id === created.project_id))}
                  </Button>
                </span>
              ) : projects.length === 0 ? (
                <span className="text-muted-foreground">Create a project before you add tasks.</span>
              ) : null}
            </div>
            <div className="flex items-center gap-3">
              <Label className="text-xs font-normal text-muted-foreground">
                <Switch
                  size="sm"
                  checked={createMore}
                  onCheckedChange={(checked) => {
                    setCreateMore(checked)
                    window.localStorage.setItem(CREATE_MORE_KEY, checked ? '1' : '0')
                  }}
                />
                Create more
              </Label>
              <Button type="submit" disabled={!canCreate} title="Ctrl/⌘ + Enter">
                {createTask.isPending ? 'Creating…' : 'Create task'}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
