import { useCommand } from '@/shortcuts/useCommand'
import { useRef, useState } from 'react'
import { toast } from 'sonner'
import { Calendar, ChevronRight, Paperclip2 as Paperclip, Refresh2, Signpost, User as UserIcon, Weight, Xmark as X } from 'reicon-react'
import { apiClient } from '@/api/client'
import { uploadTaskAttachments } from '@/api/generated/sdk.gen'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
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
import { clipboardFiles } from '@/lib/attachmentLib'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { CreateTaskBody, TaskRecord } from '@/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { DatePicker } from '@/components/common/DatePicker'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { uploadFiles } from '@/features/tasks/api/uploadQueue'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useLabels } from '@/features/tasks/api/labels'
import { useMilestones } from '@/features/tasks/api/milestones'
import { useCycles } from '@/features/tasks/api/cycles'
import { cycleName, estimateLabel, estimateOptions, openCycles } from '@/features/tasks/cyclesLib'
import { useCreateTaskFromPayload, useTemplates, type Template } from '@/features/tasks/api/intake'
import { taskIdentifier, type TaskPriority } from '@/features/tasks/api/models'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { useCreateTask } from '@/features/tasks/api/tasks'
import { dueDateLabel, PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses, statusKeyOf } from '@/features/tasks/taskMeta'
import { resolveStatusId } from '@/features/tasks/tasksLib'
import { PriorityIcon } from './PriorityIcon'
import { TaskLabels } from './TaskLabels'
import { TaskStatusIcon } from './TaskStatusIcon'
import { EditablePreview, taskTextVariants } from './TaskTextFields'

const DAY_MS = 86_400_000
const CREATE_MORE_KEY = 'orbit:new_task_create_more'

interface NewTaskDialogProps {
  /** Starting properties (the page's filter, a group header). Unknown or missing ones fall back to the first project and its default status. */
  defaults?: Partial<CreateTaskBody>
  /** Text the first task starts with (a task made from a chat message). */
  initialTitle?: string
  initialDescription?: string
  /** Called after the exit has finished. */
  onClose: () => void
  /** Opens a created task: at once without "Create more", else from the "Created" notice. */
  onOpenTask: (task: TaskRecord) => void
}

/** Creates tasks without leaving the page. Callers mount it while open. With "Create more" on, the
    dialog stays open after each task and keeps the properties, so a batch needs only titles. */
export function NewTaskDialog({ defaults = {}, initialTitle = '', initialDescription = '', onClose, onOpenTask }: NewTaskDialogProps) {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id).data ?? []
  const statuses = useAllStatuses(workspace.id, projects).data
  const members = useMembers(workspace.id).data ?? []
  const labels = useLabels(workspace.id).data ?? []
  const allMilestones = useMilestones(workspace.id).data ?? []
  const createTask = useCreateTask(workspace.id)
  const createFromPayload = useCreateTaskFromPayload(workspace.id)
  // the sub-issues of the template that filled the form; they are created with the task
  const [template, setTemplate] = useState<Template | null>(null)
  const [open, setOpen] = useState(true)
  const [title, setTitle] = useState(initialTitle)
  const [description, setDescription] = useState(initialDescription)
  const [draft, setDraft] = useState(() => ({
    projectId: defaults.project_id,
    statusId: defaults.status_id,
    priority: (defaults.priority ?? 'none') as TaskPriority,
    assigneeIds: defaults.assignee_ids ?? [],
    labelIds: defaults.label_ids ?? [],
    dueAt: defaults.due_at ?? null,
    dueStartAt: defaults.due_start_at ?? null,
    milestoneId: defaults.milestone_id ?? null,
    cycleId: defaults.cycle_id ?? null,
    estimate: defaults.estimate ?? null,
  }))
  const [createMore, setCreateMore] = useState(() => window.localStorage.getItem(CREATE_MORE_KEY) === '1')
  const [created, setCreated] = useState<TaskRecord>()
  const [dueDateOpen, setDueDateOpen] = useState(false)
  // like the task page: the description shows as rendered Markdown until it is edited
  const [editingDescription, setEditingDescription] = useState(false)
  // files wait here and upload once the task exists
  const [files, setFiles] = useState<File[]>([])
  const [uploading, setUploading] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const titleRef = useRef<HTMLInputElement>(null)
  const descriptionRef = useRef<HTMLTextAreaElement>(null)

  // projects and statuses can still load when the dialog opens outside Tasks, so the fallbacks are derived
  const project = projects.find((candidate) => candidate.id === draft.projectId) ?? projects[0]
  // a Duplicate status needs a canonical task, which a new task cannot have yet
  // Triage is offered only in a project that uses it
  const statusOptions = project
    ? projectStatuses(statuses, project.id).filter((option) => option.category !== 'duplicate' && (option.category !== 'triage' || project.triage_enabled))
    : []
  const status = statusOptions.find((option) => option.id === draft.statusId)
    ?? statusOptions.find((option) => option.id === (project ? resolveStatusId(statuses, project.id, null) : undefined))
    ?? statusOptions[0]
  const milestones = allMilestones.filter((item) => item.project_id === project?.id)
  const milestone = milestones.find((item) => item.id === draft.milestoneId)
  const cycles = openCycles(useCycles(workspace.id, project?.id).data ?? [])
  const cycle = cycles.find((item) => item.id === draft.cycleId)
  const estimates = estimateOptions(project?.estimate_scale)
  const estimate = estimates.some((option) => option.points === draft.estimate) ? draft.estimate : null
  const assignees = members.filter((member) => draft.assigneeIds.includes(member.id))
  const templates = useTemplates(workspace.id, project?.id).data ?? []
  const templateSubIssues = template && template.project_id === project?.id ? template.payload.sub_issues ?? [] : []
  const busy = createTask.isPending || createFromPayload.isPending || uploading
  const canCreate = title.trim() !== '' && project !== undefined && status !== undefined && !busy
  const attach = (added: FileList | File[] | null) => {
    if (added && added.length > 0) setFiles((current) => [...current, ...Array.from(added)])
  }

  /** A template fills the fields; the person can change each one before the save. References that no longer exist are left out. */
  const applyTemplate = (picked: Template) => {
    const { payload } = picked
    setTemplate(picked)
    setTitle(payload.title)
    setDescription(payload.description ?? '')
    setDraft((current) => ({
      ...current,
      statusId: statusOptions.find((option) => option.id === payload.status_id)?.id ?? current.statusId,
      priority: (payload.priority ?? 'none') as TaskPriority,
      assigneeIds: (payload.assignee_ids ?? []).filter((id) => members.some((member) => member.id === id)),
      labelIds: (payload.label_ids ?? []).filter((id) => labels.some((label) => label.id === id)),
      milestoneId: milestones.find((item) => item.id === payload.milestone_id)?.id ?? null,
      estimate: payload.estimate ?? null,
      dueAt: payload.due_offset_days == null ? null : new Date(Date.now() + payload.due_offset_days * DAY_MS).toISOString(),
      dueStartAt: null,
    }))
  }

  const openTask = (task: TaskRecord) => {
    setOpen(false)
    onOpenTask(task)
  }
  /** `more` keeps the dialog for the next task; it defaults to the "Create more" switch. */
  const submit = async (more = createMore) => {
    if (!canCreate) return
    try {
      // a template with sub-issues is created in one request, so it is never created in part
      const task = templateSubIssues.length > 0
        ? await createFromPayload.mutateAsync({
          projectId: project.id,
          payload: {
            title: title.trim(),
            description: description.trim(),
            status_id: status.id,
            priority: draft.priority,
            assignee_ids: draft.assigneeIds,
            label_ids: draft.labelIds,
            milestone_id: milestone?.id ?? null,
            estimate,
            due_offset_days: draft.dueAt ? Math.max(0, Math.round((Date.parse(draft.dueAt) - Date.now()) / DAY_MS)) : null,
            sub_issues: templateSubIssues,
          },
        })
        : await createTask.mutateAsync({
        title: title.trim(),
        description: description.trim(),
        project_id: project.id,
        status_id: status.id,
        priority: draft.priority,
        assignee_ids: draft.assigneeIds,
        label_ids: draft.labelIds,
        due_at: draft.dueAt,
        due_start_at: draft.dueStartAt,
        ...(milestone ? { milestone_id: milestone.id } : {}),
        ...(cycle ? { cycle_id: cycle.id } : {}),
        ...(estimate != null ? { estimate } : {}),
      })
      if (files.length > 0) {
        setUploading(true)
        try {
          await uploadFiles(files, (file) => uploadTaskAttachments({ client: apiClient, path: { workspace_id: workspace.id, task_id: task.id }, body: { file }, throwOnError: true }), () => {})
        } catch {
          // the task exists; its page can take the files again
          toast.error('The task was created, but some files were not attached.')
        } finally {
          setUploading(false)
        }
      }
      if (!more) return openTask(task)
      // the properties stay for the next task of the batch
      setCreated(task)
      setTemplate(null)
      setTitle('')
      setDescription('')
      setEditingDescription(false)
      setFiles([])
      titleRef.current?.focus()
    } catch {
      // The draft stays; the mutation shows the error beside the create action.
    }
  }

  useCommand('newTask.submitMore', () => void submit(true))

  return (
    <Dialog
      open={open}
      onOpenChange={(next, details) => {
        if (next) return
        // a request in flight must finish here, and a stray click beside the dialog must not drop a draft
        if (busy || (details.reason === 'outside-press' && (title.trim() || description.trim() || files.length > 0))) details.cancel()
        else setOpen(false)
      }}
      onOpenChangeComplete={(next) => {
        if (!next) onClose()
      }}
    >
      {/* anchored to the top so the dialog grows downward as the description grows. It always animates, also from
          the C key: a form the user settles into, a few times a day, reads as broken when it just appears. The
          entrance is shorter and smaller than the default (150ms, from 97%), so typing can start at once. */}
      <DialogContent initialFocus={titleRef} className="top-[14vh] max-h-[80vh] translate-y-0 gap-3 duration-150 data-open:zoom-in-97! data-closed:zoom-out-97! sm:max-w-2xl">
        <form
          className="contents"
          onSubmit={(event) => {
            // React events cross portals: the "New label" form in the label popover must not create the task
            if (event.target !== event.currentTarget) return
            event.preventDefault()
            void submit()
          }}
          onKeyDown={(event) => {
            // with Shift it is the "create and add another" command
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.shiftKey) {
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
            {templates.length > 0 ? (
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button type="button" variant="ghost" size="xs" className="ml-auto max-w-48 font-normal text-muted-foreground" aria-label={`Template: ${template?.name ?? 'none'}`}>
                      <span className="truncate">{template ? template.name : 'Template'}</span>
                    </Button>
                  }
                />
                <DropdownMenuContent align="end" className="w-auto min-w-45">
                  {templates.map((option) => (
                    <DropdownMenuItem key={option.id} className="data-selected:bg-accent data-selected:font-medium" data-selected={option.id === template?.id || undefined} onClick={() => applyTemplate(option)}>
                      <span className="truncate">{option.name}</span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
          </div>

          <div className="flex min-h-0 flex-col gap-1.5 overflow-y-auto">
            <Input
              ref={titleRef}
              className="h-auto rounded-none border-0 bg-transparent p-0 text-lg font-medium focus-visible:ring-0 md:text-lg dark:bg-transparent"
              value={title}
              placeholder="Task title"
              aria-label="Task title"
              readOnly={busy}
              onChange={(event) => setTitle(event.target.value)}
              onKeyDown={(event) => {
                // Enter moves on to the description; Cmd/Ctrl+Enter creates (handled by the form)
                if (event.key === 'Enter' && !event.metaKey && !event.ctrlKey) {
                  event.preventDefault()
                  setEditingDescription(true)
                  descriptionRef.current?.focus()
                }
              }}
            />
            <div
              className="rounded-lg transition-[box-shadow,background-color] data-drop-over:bg-primary/10 data-drop-over:ring-2 data-drop-over:ring-primary/40"
              data-drop-over={dropOver || undefined}
              onDragOver={(event) => {
                if (!event.dataTransfer.types.includes('Files')) return
                event.preventDefault()
                setDropOver(true)
              }}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropOver(false)
              }}
              onDrop={(event) => {
                if (!event.dataTransfer.types.includes('Files')) return
                event.preventDefault()
                setDropOver(false)
                attach(event.dataTransfer.files)
              }}
            >
              {editingDescription || !description ? (
                <Textarea
                  ref={descriptionRef}
                  className={taskTextVariants({ field: 'description', mode: 'edit', className: 'min-h-20' })}
                  data-keep-font-size=""
                  value={description}
                  placeholder="Add description… (paste or drop images and files)"
                  aria-label="Description"
                  autoFocus={editingDescription}
                  readOnly={busy}
                  onChange={(event) => setDescription(event.target.value)}
                  onFocus={() => setEditingDescription(true)}
                  onBlur={() => setEditingDescription(false)}
                  onPaste={(event) => {
                    const pasted = clipboardFiles(event)
                    if (pasted.length === 0) return
                    event.preventDefault()
                    attach(pasted)
                  }}
                />
              ) : (
                <EditablePreview className={taskTextVariants({ field: 'description', mode: 'preview', className: 'min-h-20' })} ariaLabel="Description" onEdit={() => setEditingDescription(true)}>
                  {renderMarkdownBlocks(description, 'new-task-description')}
                </EditablePreview>
              )}
              {files.length > 0 ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {files.map((file, index) => (
                    <span key={`${file.name}:${index}`} className="inline-flex h-6 max-w-56 items-center gap-1 rounded-md border pr-0.5 pl-1.5 text-xs text-muted-foreground">
                      <Paperclip aria-hidden className="size-3 shrink-0" />
                      <span className="truncate">{file.name}</span>
                      <Button type="button" variant="ghost" size="icon-xs" className="size-4 rounded-full" aria-label={`Remove ${file.name}`} disabled={busy} onClick={() => setFiles((current) => current.filter((_, at) => at !== index))}>
                        <X />
                      </Button>
                    </span>
                  ))}
                </div>
              ) : null}
              <input ref={fileInputRef} type="file" multiple hidden aria-label="Attach files" onChange={(event) => { attach(event.target.files); event.target.value = '' }} />
              <Button type="button" variant="ghost" className="mt-1.5 -ml-2 text-xs text-muted-foreground/70" disabled={busy} onClick={() => fileInputRef.current?.click()}>
                <Paperclip className="size-3.5" />
                Attach
              </Button>
            </div>
          </div>

          {templateSubIssues.length > 0 ? (
            <p className="text-xs text-muted-foreground">
              With {templateSubIssues.length} sub-issue{templateSubIssues.length === 1 ? '' : 's'} from the template: {templateSubIssues.map((item) => item.title).join(', ')}
            </p>
          ) : null}
          <TaskLabels workspaceId={workspace.id} labelIds={draft.labelIds} labels={labels} onChange={(labelIds) => setDraft((current) => ({ ...current, labelIds }))} />
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
            {milestones.length > 0 ? (
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button type="button" variant="outline" size="sm" className="max-w-48 font-normal" aria-label={`Milestone: ${milestone?.name ?? 'none'}`}>
                      <Signpost aria-hidden />
                      <span className="truncate">{milestone?.name ?? 'Milestone'}</span>
                    </Button>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-45">
                  {milestones.map((option) => (
                    <DropdownMenuItem
                      key={option.id}
                      className="data-selected:bg-accent data-selected:font-medium"
                      data-selected={option.id === milestone?.id || undefined}
                      onClick={() => setDraft((current) => ({ ...current, milestoneId: option.id }))}
                    >
                      {option.name}
                    </DropdownMenuItem>
                  ))}
                  {milestone ? <DropdownMenuItem onClick={() => setDraft((current) => ({ ...current, milestoneId: null }))}>No milestone</DropdownMenuItem> : null}
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
            {cycles.length > 0 ? (
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button type="button" variant="outline" size="sm" className="max-w-40 font-normal" aria-label={`Cycle: ${cycle ? cycleName(cycle) : 'none'}`}>
                      <Refresh2 aria-hidden />
                      <span className="truncate">{cycle ? cycleName(cycle) : 'Cycle'}</span>
                    </Button>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-45">
                  {cycles.map((option) => (
                    <DropdownMenuItem key={option.id} className="data-selected:bg-accent data-selected:font-medium" data-selected={option.id === cycle?.id || undefined} onClick={() => setDraft((current) => ({ ...current, cycleId: option.id }))}>
                      <span className="flex-1 truncate">{cycleName(option)}</span>
                      {option.state === 'current' ? <span className="text-xs text-muted-foreground">Current</span> : null}
                    </DropdownMenuItem>
                  ))}
                  {cycle ? <DropdownMenuItem onClick={() => setDraft((current) => ({ ...current, cycleId: null }))}>No cycle</DropdownMenuItem> : null}
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
            {estimates.length > 0 ? (
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button type="button" variant="outline" size="sm" className="font-normal" aria-label={`Estimate: ${estimate ?? 'none'}`}>
                      <Weight aria-hidden />
                      {estimate == null ? 'Estimate' : estimateLabel(estimate, project?.estimate_scale)}
                    </Button>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-32">
                  {estimates.map((option) => (
                    <DropdownMenuItem key={option.points} className="data-selected:bg-accent data-selected:font-medium" data-selected={option.points === estimate || undefined} onClick={() => setDraft((current) => ({ ...current, estimate: option.points }))}>
                      {option.label}
                    </DropdownMenuItem>
                  ))}
                  {estimate != null ? <DropdownMenuItem onClick={() => setDraft((current) => ({ ...current, estimate: null }))}>No estimate</DropdownMenuItem> : null}
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
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
                        <UserIcon aria-hidden="true" className="text-muted-foreground" />
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
                  onChange={({ start, end }) => setDraft((current) => ({ ...current, dueAt: end, dueStartAt: start }))}
                />
              </PopoverContent>
            </Popover>
          </div>

          <DialogFooter className="items-center py-2.5 sm:justify-between">
            <div className="min-w-0 text-xs">
              {createTask.isError || createFromPayload.isError ? (
                <span role="alert" className="text-destructive">The task was not created. Try again.</span>
              ) : created ? (
                <span role="status" className="flex items-center gap-1 text-muted-foreground">
                  Created
                  <Button type="button" variant="link" size="xs" className="px-0" onClick={() => openTask(created)}>
                    {taskIdentifier(created.id, projects.find((candidate) => candidate.id === created.project_id), created.number)}
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
              <Tip label="Ctrl/⌘ + Enter">
                <Button type="submit" disabled={!canCreate}>
                  {uploading ? 'Attaching…' : createTask.isPending ? 'Creating…' : 'Create task'}
                </Button>
              </Tip>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
