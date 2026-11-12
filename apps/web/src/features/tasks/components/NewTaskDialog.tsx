import { useCommand } from '@/shortcuts/useCommand'
import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { toast } from 'sonner'
import { Calendar, ChevronRight, Diamonds, Hierarchy2, Link2, Paperclip2 as Paperclip, RefreshCircle, Tag, Triangle, User as UserIcon, Xmark as X } from 'reicon-react'
import { apiClient } from '@/api/client'
import { createTaskRelation, updateTask } from '@orbit/api/generated/sdk.gen'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { Dialog, DialogContent, DialogFooter, DialogTitle } from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { clipboardFiles } from '@/lib/attachmentLib'
import type { CreateTaskBody, TaskRecord } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { DatePicker } from '@/components/common/DatePicker'
import { SearchPicker } from '@/components/common/SearchPicker'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { uploadInlineFile } from '@/features/tasks/api/inlineFiles'
import { fileBlock, fileUrls, isEmptyRichText, mapFileUrls } from '@/features/tasks/lib/richText'
// Type-only import: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'
import { useTeamMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useLabels } from '@/features/tasks/api/labels'
import { taskIdentifier, type Task, type TaskPriority } from '@/features/tasks/api/models'
import { useAllStatuses, useProjectAudience, useProjects } from '@/features/tasks/api/projects'
import { useCreateTask } from '@/features/tasks/api/tasks'
import { useCycles } from '@/features/tasks/api/cycles'
import { useMilestones } from '@/features/tasks/api/milestones'
import { cycleName, estimateLabel } from '@/features/tasks/planningLib'
import { CyclePicker, EstimatePicker, MilestonePicker } from './PlanningPickers'
import { dueDateLabel, matchingLabelIds, PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses, statusKeyOf } from '@/features/tasks/taskMeta'
import { resolveStatusId } from '@/features/tasks/tasksLib'
import { ADD_RELATION_OPTIONS, type RelationKind } from '@/features/tasks/relationsLib'
import { EditorFallback, PageEditor } from './lazyEditor'
import { personOption, projectOption } from './pickerOptions'
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
  const members = useTeamMembers(workspace.id).data ?? []
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
    sourceUrl: defaults.source_url ?? null,
    milestoneId: defaults.milestone_id ?? null,
    cycleId: defaults.cycle_id ?? null,
    estimate: defaults.estimate ?? null,
  }))
  // relations are saved once the task exists
  const [relations, setRelations] = useState<PendingRelation[]>([])
  const [picker, setPicker] = useState<'parent' | PendingRelation['kind'] | null>(null)
  const [createMore, setCreateMore] = useState(() => window.localStorage.getItem(CREATE_MORE_KEY) === '1')
  const [created, setCreated] = useState<TaskRecord>()
  const [dueDateOpen, setDueDateOpen] = useState(false)
  const [sourceOpen, setSourceOpen] = useState(false)
  // files wait here and upload once the task exists
  const [files, setFiles] = useState<File[]>([])
  const [uploading, setUploading] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const titleRef = useRef<HTMLInputElement>(null)
  const descriptionRef = useRef<PageEditorHandle>(null)
  // images and files picked in the description (/image, /file) stay in the browser, by `blob:` URL, until the task exists
  const held = useRef(new Map<string, File>())
  const holdFile = useCallback(async (file: File) => {
    const url = URL.createObjectURL(file)
    held.current.set(url, file)
    return url
  }, [])
  const releaseHeld = useCallback(() => {
    for (const url of held.current.keys()) URL.revokeObjectURL(url)
    held.current.clear()
  }, [])
  useEffect(() => releaseHeld, [releaseHeld])
  const fileScope = useMemo(() => ({ workspaceId: workspace.id, taskId: '', deferred: true }), [workspace.id])

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
  const audience = useProjectAudience(workspace.id, project, members)
  // a milestone or cycle belongs to one project: another project drops it
  const milestones = useMilestones(workspace.id, project?.id).data ?? []
  const cycles = useCycles(workspace.id, project?.id).data ?? []
  const milestone = milestones.find((item) => item.id === draft.milestoneId)
  const cycle = cycles.find((item) => item.id === draft.cycleId)
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
      // the held files are not on the server yet: the new task starts without their blocks' URLs
      const description = mapFileUrls(content, (url) => (held.current.has(url) ? '' : url))
      const task = await createTask.mutateAsync({
        title: title.trim(),
        ...(isEmptyRichText(description) ? {} : { description_json: description }),
        project_id: project.id,
        status_id: status.id,
        priority: draft.priority,
        assignee_ids: draft.assigneeIds,
        label_ids: labelIds,
        due_at: draft.dueAt,
        due_start_at: draft.dueStartAt,
        parent_task_id: draft.parent?.id ?? null,
        milestone_id: milestone?.id ?? null,
        cycle_id: cycle?.id ?? null,
        estimate: draft.estimate,
        ...(draft.sourceUrl ? { source_url: draft.sourceUrl } : {}),
      })
      if (relations.length > 0) {
        const results = await Promise.allSettled(relations.map((relation) => createTaskRelation({ client: apiClient, path: { workspace_id: workspace.id, task_id: task.id }, body: { type: relation.kind, task_id: relation.task.id }, throwOnError: true })))
        if (results.some((result) => result.status === 'rejected')) toast.error('The task was created, but some relations were not added.')
      }
      const heldUrls = [...new Set(fileUrls(content).filter((url) => held.current.has(url)))]
      if (files.length > 0 || heldUrls.length > 0) {
        // the held files go in at their blocks; the chip files go after the text
        setUploading(true)
        try {
          const uploaded = new Map<string, string>()
          for (const url of heldUrls) uploaded.set(url, await uploadInlineFile(workspace.id, task.id, held.current.get(url)!))
          const blocks: Array<Record<string, unknown>> = []
          for (const file of files) blocks.push(fileBlock(file, await uploadInlineFile(workspace.id, task.id, file)))
          const text = isEmptyRichText(content) ? [] : mapFileUrls(content, (url) => uploaded.get(url) ?? url)
          await updateTask({ client: apiClient, path: { workspace_id: workspace.id, task_id: task.id }, body: { expected_version: task.version, description_json: [...text, ...blocks] }, throwOnError: true })
        } catch {
          // the task exists; its page can take the files again
          toast.error('The task was created, but some files were not added.')
        } finally {
          setUploading(false)
        }
      }
      releaseHeld()
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
            <SearchPicker
              label="Project"
              placeholder="Change project…"
              emptyText="No projects match"
              options={projects.map(projectOption)}
              selected={project ? [project.id] : []}
              // the status follows by name into the other project's workflow
              onSelect={(id) => setDraft((current) => ({ ...current, projectId: id, statusId: resolveStatusId(statuses, id, status ? statusKeyOf(status) : null) }))}
              trigger={
                <Button type="button" variant="outline" size="xs" className="max-w-48 font-normal" aria-label={`Project: ${project?.name ?? 'none'}`}>
                  <ColorDot color={project?.color} />
                  <span className="truncate">{project?.name ?? 'No project'}</span>
                </Button>
              }
            />
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
                    // no task yet: images of other tasks in the workspace stay (by link); uploads wait for the task
                    fileScope={fileScope}
                    uploadFile={holdFile}
                    placeholder="description"
                    initialMarkdown={descriptionKey === 0 ? initialDescription : undefined}
                    editable={!busy}
                    onChange={(content) => setHasDescription(!isEmptyRichText(content))}
                    submitShortcut={() => void submit()}
                    mentions={{ members, candidates: audience.filter((member) => !member.suspended), privatePage: false }}
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
            <SearchPicker
              multiple
              label="Assignees"
              placeholder="Assign to…"
              emptyText="No people match"
              options={audience.map(personOption)}
              selected={draft.assigneeIds}
              onSelect={(id) => setDraft((current) => ({ ...current, assigneeIds: current.assigneeIds.includes(id) ? current.assigneeIds.filter((item) => item !== id) : [...current.assigneeIds, id] }))}
              trigger={
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
            {milestones.length > 0 ? (
              <MilestonePicker
                milestones={milestones}
                value={milestone?.id ?? null}
                onChange={(milestoneId) => setDraft((current) => ({ ...current, milestoneId }))}
                trigger={
                  <Button type="button" variant="outline" size="sm" className="max-w-56 font-normal" aria-label={`Milestone: ${milestone?.name ?? 'none'}`}>
                    <Diamonds aria-hidden="true" className="text-muted-foreground" />
                    <span className="truncate">{milestone?.name ?? 'Milestone'}</span>
                  </Button>
                }
              />
            ) : null}
            {cycles.length > 0 ? (
              <CyclePicker
                cycles={cycles}
                value={cycle?.id ?? null}
                onChange={(cycleId) => setDraft((current) => ({ ...current, cycleId }))}
                trigger={
                  <Button type="button" variant="outline" size="sm" className="max-w-56 font-normal" aria-label={`Cycle: ${cycle ? cycleName(cycle) : 'none'}`}>
                    <RefreshCircle aria-hidden="true" className="text-muted-foreground" />
                    <span className="truncate">{cycle ? cycleName(cycle) : 'Cycle'}</span>
                  </Button>
                }
              />
            ) : null}
            <EstimatePicker
              value={draft.estimate}
              onChange={(estimate) => setDraft((current) => ({ ...current, estimate }))}
              trigger={
                <Button type="button" variant="outline" size="sm" className="font-normal" aria-label={`Estimate: ${draft.estimate === null ? 'none' : estimateLabel(draft.estimate)}`}>
                  <Triangle aria-hidden="true" className="text-muted-foreground" />
                  {draft.estimate === null ? 'Estimate' : estimateLabel(draft.estimate)}
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
            <Popover open={sourceOpen} onOpenChange={setSourceOpen}>
              <PopoverTrigger
                render={
                  <Button type="button" variant="outline" size="sm" className="max-w-56 font-normal" aria-label={draft.sourceUrl ? `Source: ${draft.sourceUrl}` : 'Source'}>
                    <Link2 aria-hidden="true" className="text-muted-foreground" />
                    <span className="truncate">{draft.sourceUrl ? draft.sourceUrl.replace(/^https?:\/\//, '').split('/')[0] : 'Source'}</span>
                  </Button>
                }
              />
              <PopoverContent align="start" className="w-72 gap-2 p-3">
                {/* its submit also reaches the task form through the portal, which ignores it */}
                <form
                  className="flex flex-col gap-2"
                  onSubmit={(event) => {
                    event.preventDefault()
                    const sourceUrl = new FormData(event.currentTarget).get('source_url')?.toString().trim() || null
                    setDraft((current) => ({ ...current, sourceUrl }))
                    setSourceOpen(false)
                  }}
                >
                  <label htmlFor="new-task-source-url" className="text-xs font-medium">Source URL</label>
                  <Input id="new-task-source-url" name="source_url" type="url" placeholder="https://..." defaultValue={draft.sourceUrl ?? ''} />
                  <Button type="submit" size="sm">Save</Button>
                </form>
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
