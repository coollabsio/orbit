import { useRef, useState } from 'react'
import { toast } from 'sonner'
import { Calendar, ChevronRight, Paperclip2 as Paperclip, User as UserIcon, Xmark as X } from 'reicon-react'
import { apiClient } from '@/api/client'
import { uploadTaskAttachments } from '@/api/generated/sdk.gen'
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
import { taskIdentifier, type TaskPriority } from '@/features/tasks/api/models'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { useCreateTask } from '@/features/tasks/api/tasks'
import { dueDateLabel, PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses, statusKeyOf } from '@/features/tasks/taskMeta'
import { resolveStatusId } from '@/features/tasks/tasksLib'
import { PriorityIcon } from './PriorityIcon'
import { TaskLabels } from './TaskLabels'
import { TaskStatusIcon } from './TaskStatusIcon'
import { EditablePreview, taskTextVariants } from './TaskTextFields'

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
  const statusOptions = project ? projectStatuses(statuses, project.id).filter((option) => option.category !== 'duplicate') : []
  const status = statusOptions.find((option) => option.id === draft.statusId)
    ?? statusOptions.find((option) => option.id === (project ? resolveStatusId(statuses, project.id, null) : undefined))
    ?? statusOptions[0]
  const assignees = members.filter((member) => draft.assigneeIds.includes(member.id))
  const busy = createTask.isPending || uploading
  const canCreate = title.trim() !== '' && project !== undefined && status !== undefined && !busy
  const attach = (added: FileList | File[] | null) => {
    if (added && added.length > 0) setFiles((current) => [...current, ...Array.from(added)])
  }

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
      if (!createMore) return openTask(task)
      // the properties stay for the next task of the batch
      setCreated(task)
      setTitle('')
      setDescription('')
      setEditingDescription(false)
      setFiles([])
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
        if (busy || (details.reason === 'outside-press' && (title.trim() || description.trim() || files.length > 0))) details.cancel()
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
            // React events cross portals: the "New label" form in the label popover must not create the task
            if (event.target !== event.currentTarget) return
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
                  onDone={({ start, end }) => {
                    setDraft((current) => ({ ...current, dueAt: end, dueStartAt: start }))
                    setDueDateOpen(false)
                  }}
                />
              </PopoverContent>
            </Popover>
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
                {uploading ? 'Attaching…' : createTask.isPending ? 'Creating…' : 'Create task'}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
