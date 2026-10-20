import { useCommand } from '@/shortcuts/useCommand'
import { Suspense, useRef, useState } from 'react'
import { toast } from 'sonner'
import { Calendar, ChevronRight, Hierarchy2, Paperclip2 as Paperclip, Tag, User as UserIcon, Xmark as X } from 'reicon-react'
import { apiClient } from '@/api/client'
import { createTaskRelation, updateTask } from '@/api/generated/sdk.gen'
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
import { clipboardFiles } from '@/lib/attachmentLib'
import type { CreateTaskBody, TaskRecord } from '@/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { DatePicker } from '@/components/common/DatePicker'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { uploadInlineFile } from '@/features/tasks/api/inlineFiles'
import { fileBlock, isEmptyRichText } from '@/features/tasks/lib/richText'
// Type-only import: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useLabels } from '@/features/tasks/api/labels'
import { taskIdentifier, type Task, type TaskPriority } from '@/features/tasks/api/models'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { useCreateTask } from '@/features/tasks/api/tasks'
import { dueDateLabel, matchingLabelIds, PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses, statusKeyOf } from '@/features/tasks/taskMeta'
import { resolveStatusId } from '@/features/tasks/tasksLib'
import { ADD_RELATION_OPTIONS, type RelationKind } from '@/features/tasks/relationsLib'
import { EditorFallback, PageEditor } from './lazyEditor'
import { PriorityIcon } from './PriorityIcon'
import { LabelPicker } from './TaskLabels'
import { TaskPickerDialog } from './TaskPickerDialog'
import { AddRelationMenu } from './TaskRelations'
import { TaskStatusIcon } from './TaskStatusIcon'

const CREATE_MORE_KEY = 'orbit:new_task_create_more'
// a duplicate needs a Duplicate status, which a new task cannot have
const RELATION_OPTIONS = ADD_RELATION_OPTIONS.filter((option) => option.kind !== 'duplicate')
const RELATION_LABEL: Record<RelationKind, string> = { blocks: 'Blocks', blocked_by: 'Blocked by', related: 'Related to', duplicate: 'Duplicate of' }

type PendingRelation = { kind: Exclude<RelationKind, 'duplicate'>; task: Task }

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
  const labelsQuery = useLabels(workspace.id)
  const labels = labelsQuery.data ?? []
  const createTask = useCreateTask(workspace.id)
  const [open, setOpen] = useState(true)
  const [title, setTitle] = useState(initialTitle)
  /** Whether the description says anything (the draft guard); the editor holds the text itself. */
  const [hasDescription, setHasDescription] = useState(initialDescription.trim() !== '')
  /** Bumped to start the next task of a batch with an empty description. */
  const [descriptionKey, setDescriptionKey] = useState(0)
  const [draft, setDraft] = useState(() => ({
    projectId: defaults.project_id,
    statusId: defaults.status_id,
    priority: (defaults.priority ?? 'none') as TaskPriority,
    assigneeIds: defaults.assignee_ids ?? [],
    labelIds: defaults.label_ids ?? [],
    dueAt: defaults.due_at ?? null,
    dueStartAt: defaults.due_start_at ?? null,
    parent: null as Task | null,
  }))
  // relations are saved once the task exists
  const [relations, setRelations] = useState<PendingRelation[]>([])
  const [picker, setPicker] = useState<'parent' | PendingRelation['kind'] | null>(null)
  const [createMore, setCreateMore] = useState(() => window.localStorage.getItem(CREATE_MORE_KEY) === '1')
  const [created, setCreated] = useState<TaskRecord>()
  const [dueDateOpen, setDueDateOpen] = useState(false)
  // files wait here and upload once the task exists
  const [files, setFiles] = useState<File[]>([])
  const [uploading, setUploading] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const titleRef = useRef<HTMLInputElement>(null)
  const descriptionRef = useRef<PageEditorHandle>(null)

  // projects and statuses can still load when the dialog opens outside Tasks, so the fallbacks are derived
  const project = projects.find((candidate) => candidate.id === draft.projectId) ?? projects[0]
  // a Duplicate status needs a canonical task, which a new task cannot have yet
  const statusOptions = project ? projectStatuses(statuses, project.id).filter((option) => option.category !== 'duplicate') : []
  const status = statusOptions.find((option) => option.id === draft.statusId)
    ?? statusOptions.find((option) => option.id === (project ? resolveStatusId(statuses, project.id, null) : undefined))
    ?? statusOptions[0]
  // labels follow by name into another project, like a task that moves; the server checks them until they load
  const labelIds = project && labelsQuery.isSuccess ? matchingLabelIds(labels, draft.labelIds, project.id) : draft.labelIds
  const chosenLabels = labels.filter((label) => labelIds.includes(label.id)).sort((a, b) => a.name.localeCompare(b.name))
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
  /** `more` keeps the dialog for the next task; it defaults to the "Create more" switch. */
  const submit = async (more = createMore) => {
    if (!canCreate) return
    try {
      const content = (descriptionRef.current?.getContent() ?? []) as Array<Record<string, unknown>>
      const task = await createTask.mutateAsync({
        title: title.trim(),
        ...(isEmptyRichText(content) ? {} : { description_json: content }),
        project_id: project.id,
        status_id: status.id,
        priority: draft.priority,
        assignee_ids: draft.assigneeIds,
        label_ids: labelIds,
        due_at: draft.dueAt,
        due_start_at: draft.dueStartAt,
        parent_task_id: draft.parent?.id ?? null,
      })
      if (relations.length > 0) {
        const results = await Promise.allSettled(relations.map((relation) => createTaskRelation({ client: apiClient, path: { workspace_id: workspace.id, task_id: task.id }, body: { type: relation.kind, task_id: relation.task.id }, throwOnError: true })))
        if (results.some((result) => result.status === 'rejected')) toast.error('The task was created, but some relations were not added.')
      }
      if (files.length > 0) {
        // the files go into the description, after the text
        setUploading(true)
        try {
          const blocks: Array<Record<string, unknown>> = []
          for (const file of files) blocks.push(fileBlock(file, await uploadInlineFile(workspace.id, task.id, file)))
          const text = isEmptyRichText(content) ? [] : content
          await updateTask({ client: apiClient, path: { workspace_id: workspace.id, task_id: task.id }, body: { expected_version: task.version, description_json: [...text, ...blocks] }, throwOnError: true })
        } catch {
          // the task exists; its page can take the files again
          toast.error('The task was created, but some files were not added.')
        } finally {
          setUploading(false)
        }
      }
      if (!more) return openTask(task)
      // the properties stay for the next task of the batch
      setCreated(task)
      setTitle('')
      setHasDescription(false)
      setDescriptionKey((key) => key + 1)
      setFiles([])
      setRelations([])
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
        if (busy || (details.reason === 'outside-press' && (title.trim() || hasDescription || files.length > 0))) details.cancel()
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
              {/* files dropped or pasted on the description wait as chips and go into it once the task exists */}
              <div
                className="min-h-20"
                onPasteCapture={(event) => {
                  const pasted = clipboardFiles(event)
                  if (pasted.length === 0) return
                  event.preventDefault()
                  event.stopPropagation()
                  attach(pasted)
                }}
                onDropCapture={(event) => {
                  if (!event.dataTransfer.types.includes('Files')) return
                  event.preventDefault()
                  event.stopPropagation()
                  setDropOver(false)
                  attach(event.dataTransfer.files)
                }}
              >
                <Suspense fallback={<EditorFallback />}>
                  <PageEditor
                    key={descriptionKey}
                    ref={descriptionRef}
                    pageId={`new-task:${descriptionKey}`}
                    profile="task"
                    // no task yet: images of other tasks in the workspace stay (by link); nothing uploads here
                    fileScope={{ workspaceId: workspace.id, taskId: '' }}
                    placeholder="description"
                    initialMarkdown={descriptionKey === 0 ? initialDescription : undefined}
                    editable={!busy}
                    onChange={(content) => setHasDescription(!isEmptyRichText(content))}
                    submitShortcut={() => void submit()}
                    mentions={{ members, candidates: members.filter((member) => !member.suspended), privatePage: false }}
                    sideMenu={false}
                    ariaLabel="New task description"
                  />
                </Suspense>
              </div>
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
              {relations.length > 0 ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {relations.map((relation) => (
                    <span key={`${relation.kind}:${relation.task.id}`} className="inline-flex h-6 max-w-72 items-center gap-1 rounded-md border pr-0.5 pl-1.5 text-xs">
                      <span className="shrink-0 text-muted-foreground">{RELATION_LABEL[relation.kind]}</span>
                      <span className="shrink-0 text-muted-foreground tabular-nums">{relation.task.identifier}</span>
                      <span className="truncate">{relation.task.title || 'Untitled'}</span>
                      <Button type="button" variant="ghost" size="icon-xs" className="size-4 rounded-full" aria-label={`Remove relation to ${relation.task.identifier}`} disabled={busy} onClick={() => setRelations((current) => current.filter((item) => item !== relation))}>
                        <X />
                      </Button>
                    </span>
                  ))}
                </div>
              ) : null}
              <div className="mt-1.5 -ml-2 flex items-center">
                <Button type="button" variant="ghost" className="text-xs text-muted-foreground/70" disabled={busy} onClick={() => fileInputRef.current?.click()}>
                  <Paperclip className="size-3.5" />
                  Attach
                </Button>
                <AddRelationMenu options={RELATION_OPTIONS} onChoose={(kind) => setPicker(kind as PendingRelation['kind'])} />
              </div>
            </div>
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
            <LabelPicker
              workspaceId={workspace.id}
              projectId={project?.id ?? ''}
              labelIds={labelIds}
              labels={labels}
              onChange={(labelIds) => setDraft((current) => ({ ...current, labelIds }))}
              trigger={
                <Button type="button" variant="outline" size="sm" className="max-w-56 font-normal" disabled={!project} aria-label={chosenLabels.length > 0 ? `Labels: ${chosenLabels.map((label) => label.name).join(', ')}` : 'Labels'}>
                  {chosenLabels.length > 0 ? (
                    <>
                      <span aria-hidden="true" className="flex -space-x-0.5">
                        {chosenLabels.map((label) => <ColorDot key={label.id} color={label.color} className="ring-2 ring-background" />)}
                      </span>
                      <span className="truncate">{chosenLabels.map((label) => label.name).join(', ')}</span>
                    </>
                  ) : (
                    <>
                      <Tag aria-hidden="true" className="text-muted-foreground" />
                      Labels
                    </>
                  )}
                </Button>
              }
            />
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
            {draft.parent ? (
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button type="button" variant="outline" size="sm" className="max-w-64 font-normal" aria-label={`Parent: ${draft.parent.identifier} ${draft.parent.title}`}>
                      <Hierarchy2 aria-hidden="true" />
                      <span className="shrink-0 text-muted-foreground tabular-nums">{draft.parent.identifier}</span>
                      <span className="truncate">{draft.parent.title || 'Untitled'}</span>
                    </Button>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-45">
                  <DropdownMenuItem onClick={() => setPicker('parent')}>Change parent…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setDraft((current) => ({ ...current, parent: null }))}>Remove parent</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            ) : (
              <Button type="button" variant="outline" size="sm" className="font-normal" onClick={() => setPicker('parent')}>
                <Hierarchy2 aria-hidden="true" className="text-muted-foreground" />
                Parent
              </Button>
            )}
          </div>

          <DialogFooter className="items-center py-2.5 sm:justify-between">
            <div className="min-w-0 text-xs">
              {createTask.isError ? (
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
        {picker ? (
          <TaskPickerDialog
            open
            onOpenChange={(next) => { if (!next) setPicker(null) }}
            title={picker === 'parent' ? 'New task: Parent…' : `New task: ${RELATION_LABEL[picker]}…`}
            statuses={statuses}
            excludeIds={picker === 'parent' ? [] : relations.map((relation) => relation.task.id)}
            onSelect={(target) => {
              if (picker === 'parent') setDraft((current) => ({ ...current, parent: target }))
              else setRelations((current) => [...current, { kind: picker, task: target }])
            }}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
