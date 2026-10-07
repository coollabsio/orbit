import { useCommand } from '@/shortcuts/useCommand'
import { useEffect, useRef, useState, type ComponentProps, type ReactNode } from 'react'
import { cn } from 'cn'
import { useQueryClient } from '@tanstack/react-query'
import { confirmAction } from '@/components/common/confirmAction'
import { ArrowLeft, Calendar, Hierarchy2, Link2, Paperclip2 as Paperclip, TaskSquare as SquareCheck, User as UserIcon, Xmark as X } from 'reicon-react'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { Input } from '@/components/ui/input'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { DatePicker } from '@/components/common/DatePicker'
import { useTaskPickerUpdate } from '@/features/tasks/useTaskPickerUpdate'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader } from '@/components/common/Pane'
import { PriorityIcon } from './PriorityIcon'
import { TaskStatusIcon } from './TaskStatusIcon'
import { dueDateLabel, PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses } from '@/features/tasks/taskMeta'
import { refIdentifier, type Project, type Task, type TaskViewState } from '@/features/tasks/api/models'
import { cachedDescendantIds, type PageTaskRecord } from '@/features/tasks/api/optimistic'
import { useProjects } from '@/features/tasks/api/projects'
import {
  useAddTaskRelation, useCreateTaskComment, useDeleteTask, useDeleteTaskAttachment, useRemoveTaskRelation,
  subIssuesQuery, useSubIssues, useTaskGithubLinks, useTaskRelations, useUpdateTask, useUploadTaskAttachments,
} from '@/features/tasks/api/tasks'
import { pickerTitle, relatedTaskIds, type RelationKind } from '@/features/tasks/relationsLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { useParentActions } from '@/features/tasks/useParentActions'
import { useMoveToProject } from '@/features/tasks/useMoveToProject'
import { ColorDot } from '@/components/common/ColorDot'
import { descendantCount, parentPickerTitle, trashConfirmDescription } from '@/features/tasks/subIssuesLib'
import { TaskPickerDialog } from './TaskPickerDialog'
import { AddRelationMenu, DuplicateBanner, TaskRelationsSection } from './TaskRelations'
import { TaskBreadcrumb } from './TaskBreadcrumb'
import { SubIssuesSection } from './TaskSubIssues'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { Attachments } from '@/components/common/Attachments'
import { ActivityFeed } from './ActivityFeed'
import { TaskCommentComposer } from './TaskCommentComposer'
import { LabelPill, TaskLabels } from './TaskLabels'
import { TaskTextFields } from './TaskTextFields'

interface TaskDetailProps {
  task: Task | undefined
  project: Project | undefined
  state: TaskViewState
  onBack: () => void
  /** Opens another task (banner, relation rows, activity links). */
  onOpenTask?: (taskId: string) => void
  /** Opens a project's task list (the breadcrumb's project crumb). */
  onOpenProject?: (projectId: string) => void
}

/** Full-page task view: main column (title, description, activity, comment composer) + properties column. */
export function TaskDetail({ task, project, state, onBack, onOpenTask, onOpenProject }: TaskDetailProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const uploadAttachments = useUploadTaskAttachments(workspace.id, task?.id ?? '')
  const deleteAttachment = useDeleteTaskAttachment(workspace.id, task?.id ?? '')
  const createComment = useCreateTaskComment(workspace.id, task?.id ?? '')
  const deleteTask = useDeleteTask(workspace.id)
  const githubLinks = useTaskGithubLinks(workspace.id, task?.id)
  const githubContentReadOnly = !githubLinks.isSuccess || githubLinks.data.some((link) => link.source)
  const githubSyncPaused = githubLinks.data?.some((link) => link.source && link.state === 'paused')
  const githubPullRequests = githubLinks.data?.filter((link) => link.kind === 'pull_request' && !link.source) ?? []
  // the task is the GitHub issue or pull request it syncs from: the server refuses to move it to another project
  const githubIssueLinked = githubLinks.data?.some((link) => link.source) ?? false
  const moveToProject = useMoveToProject(workspace.id)
  const users = state.users
  const fileInputRef = useRef<HTMLInputElement>(null)
  // the date panel needs to close itself from Clear, so the popover stays controlled
  const [dueDateOpen, setDueDateOpen] = useState(false)
  // each pick saves and the picker stays open; quick picks queue behind the save in flight
  const saveDueDate = useTaskPickerUpdate(task, 'Due date update failed.')
  const [sourceOpen, setSourceOpen] = useState(false)
  const projects = useProjects(workspace.id).data ?? []
  const relations = useTaskRelations(workspace.id, task?.id).data ?? []
  const addRelation = useAddTaskRelation(workspace.id, task?.id ?? '')
  const removeRelation = useRemoveTaskRelation(workspace.id, task?.id ?? '')
  const duplicates = useDuplicateActions(workspace.id)
  const [picker, setPicker] = useState<RelationKind | null>(null)
  const [newRelationId, setNewRelationId] = useState<string | null>(null)
  const [unmarking, setUnmarking] = useState(false)
  const queryClient = useQueryClient()
  const parentActions = useParentActions(workspace.id)
  const [composingSubIssue, setComposingSubIssue] = useState(false)
  // Esc closes the composer: focus returns to the section's "+" while the section stays (it has sub-issues),
  // else to the "Add sub-issue" button beside Attach, which comes back once the composer and section are gone
  const subIssueAddRef = useRef<HTMLButtonElement>(null)
  const subIssueHeaderAddRef = useRef<HTMLButtonElement>(null)
  const restoreSubIssueFocus = useRef(false)
  useEffect(() => {
    if (composingSubIssue || !restoreSubIssueFocus.current) return
    restoreSubIssueFocus.current = false
    ;(subIssueHeaderAddRef.current ?? subIssueAddRef.current)?.focus()
  }, [composingSubIssue])
  const [parentPicker, setParentPicker] = useState<'set' | 'add' | null>(null)
  const childIds = (useSubIssues(workspace.id, task?.id).data?.items ?? []).map((record) => record.id)
  // the banner animates only when the task becomes a duplicate while open, never on page load;
  // captured once the task has loaded, so a cold load does not count as a change
  const [initialDuplicate, setInitialDuplicate] = useState<{ taskId: string; duplicateOfId: string | null } | null>(null)
  if (task && initialDuplicate?.taskId !== task.id) setInitialDuplicate({ taskId: task.id, duplicateOfId: task.duplicateOf?.id ?? null })
  const openTask = (taskId: string) => onOpenTask?.(taskId)
  const choose = (target: Task) => {
    if (!task || !picker) return
    if (picker === 'duplicate') void duplicates.markOne(task, target)
    else addRelation.mutate({ type: picker, task_id: target.id }, { onSuccess: (record) => setNewRelationId(record.id) })
  }
  const unmark = async () => {
    if (!task) return
    setUnmarking(true)
    await duplicates.unmarkOne(task)
    setUnmarking(false)
  }
  const attach = (files: FileList | File[] | null) => {
    if (task && files && files.length > 0) uploadAttachments.mutate(Array.from(files))
  }
  const status = state.statuses.find((s) => s.id === task?.statusId)
  const statusOptions = task ? projectStatuses(state.statuses, task.projectId) : []
  const assignees = users.filter((u) => task?.assigneeIds.includes(u.id))
  const deleteAndClose = async (input: { taskId: string; version: number }) => {
    try {
      await deleteTask.mutateAsync(input)
      onBack()
    } catch {
      // The visible mutation alert keeps the user on this task and offers retry.
    }
  }

  const trash = async () => {
    if (!task) return
    // the whole subtree goes to trash with it: count every loaded level (the Sub-issues section loads them)
    const below = descendantCount(task.subIssueCount ?? 0, (id) => queryClient.getQueryData<PageTaskRecord>(subIssuesQuery(workspace.id, id).queryKey)?.items, task.id)
    if (!await confirmAction({ title: `Move ${task.identifier} to trash?`, description: trashConfirmDescription(below), confirmLabel: 'Move to trash', danger: true })) return
    void deleteAndClose({ taskId: task.id, version: task.version })
  }
  useCommand('task.trash', task ? () => void trash() : null)
  useCommand('task.addSubIssue', task ? () => setComposingSubIssue(true) : null)
  const [relationMenuOpen, setRelationMenuOpen] = useState(false)
  useCommand('task.addRelation', task ? () => setRelationMenuOpen(true) : null)

  return (
    <Pane>
      <PaneHeader>
        <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70 min-[900px]:hidden" onClick={onBack} aria-label="Back to tasks">
          <ArrowLeft className="size-4" />
        </Button>
        <TaskBreadcrumb project={project} ancestors={task?.ancestors ?? []} identifier={task?.identifier ?? 'Task'} onOpen={openTask} onOpenProject={onOpenProject} />
        {githubSyncPaused ? <Badge variant="secondary">GitHub sync paused</Badge> : null}
        <div className="flex-1" />
        {task ? <Tip label="Move to trash" side="bottom"><Button variant="destructive" disabled={deleteTask.isPending} onClick={() => void trash()}>Delete</Button></Tip> : null}
        <Tip label="Close task" side="bottom">
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70 max-[899px]:hidden" onClick={onBack} aria-label="Close task">
            <X className="size-4" />
          </Button>
        </Tip>
      </PaneHeader>
      {!task ? (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <EmptyState
            icon={SquareCheck}
            title="Task not found"
            description="This task does not exist or was removed."
          />
        </div>
      ) : (
        // the content sits centered in the space left of the properties column, which runs down the right edge;
        // the first row is as tall as its content, so a short task leaves no gap above the activity
        <div className="grid min-h-0 flex-1 overflow-y-auto min-[900px]:grid-cols-[minmax(0,1fr)_280px] min-[900px]:grid-rows-[auto_1fr] max-[899px]:grid-cols-1 max-[899px]:content-start max-[899px]:gap-[14px] max-[899px]:px-3.5 max-[899px]:pt-4 max-[899px]:pb-7">
          <div className="min-w-0 min-[900px]:mx-auto min-[900px]:w-full min-[900px]:max-w-[840px] min-[900px]:px-10 min-[900px]:pt-8">
            {task.duplicateOf ? (
              <DuplicateBanner
                duplicateOf={task.duplicateOf}
                projects={projects}
                status={status}
                animate={initialDuplicate?.taskId === task.id && task.duplicateOf.id !== initialDuplicate.duplicateOfId}
                pending={unmarking}
                onOpen={openTask}
                onUnmark={() => void unmark()}
              />
            ) : null}
            <TaskTextFields
              task={task}
              readOnly={githubContentReadOnly}
              onUpdate={(body) => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, ...body } })}
              onAttachFiles={attach}
            >
              {task.attachments.length > 0 ? (
                <div className="max-w-[520px]">
                  <Attachments attachments={task.attachments} onRemove={(id) => deleteAttachment.mutate(id)} />
                </div>
              ) : null}
              <div className="mt-1.5">
                <input ref={fileInputRef} type="file" multiple hidden aria-label="Attach files" onChange={(e) => attach(e.target.files)} />
                <Button variant="ghost" className="-ml-2 text-xs text-muted-foreground/70" onClick={() => fileInputRef.current?.click()}>
                  <Paperclip className="size-3.5" />
                  Attach
                </Button>
                <AddRelationMenu open={relationMenuOpen} onOpenChange={setRelationMenuOpen} onChoose={setPicker} />
                {(task.subIssueCount ?? 0) === 0 && childIds.length === 0 && !composingSubIssue ? (
                  <Button ref={subIssueAddRef} variant="ghost" className="text-xs text-muted-foreground/70" onClick={() => setComposingSubIssue(true)}>
                    <Hierarchy2 className="size-3.5" aria-hidden="true" />
                    Add sub-issue
                  </Button>
                ) : null}
                {uploadAttachments.isPending ? <span role="status" aria-live="polite" className="text-xs text-muted-foreground/70 tabular-nums">Uploading {uploadAttachments.progress}%</span> : null}
                {uploadAttachments.isError ? <span role="alert" className="text-xs text-destructive">{uploadAttachments.remainingCount} file(s) remain. <Button variant="ghost" onClick={uploadAttachments.retry}>Retry upload</Button></span> : null}
              </div>
            </TaskTextFields>
            {githubLinks.data?.some((link) => link.source) ? <p className="mt-2 text-xs text-muted-foreground">This task's title and description are read-only while it is linked to GitHub.</p> : null}
            {githubSyncPaused ? <p className="mt-2 text-xs text-muted-foreground">Add the project label to the GitHub issue or pull request again to resume updates.</p> : null}
            {updateTask.isError ? <p role="alert" className="text-xs text-destructive">Task update failed. <Button variant="ghost" onClick={() => updateTask.variables && updateTask.mutate(updateTask.variables)}>Retry</Button></p> : null}
            {deleteAttachment.isError ? <p role="alert" className="text-xs text-destructive">Attachment removal failed. <Button variant="ghost" onClick={() => deleteAttachment.variables && deleteAttachment.mutate(deleteAttachment.variables)}>Retry</Button></p> : null}
            {deleteTask.isError ? <p role="alert" className="text-xs text-destructive">Task deletion failed. <Button variant="ghost" onClick={() => deleteTask.variables && void deleteAndClose(deleteTask.variables)}>Retry</Button></p> : null}
            {addRelation.isError ? <p role="alert" className="text-xs text-destructive">Couldn't add the relation. {addRelation.error.message}</p> : null}
            {removeRelation.isError ? <p role="alert" className="text-xs text-destructive">Couldn't remove the relation. {removeRelation.error.message}</p> : null}

            {githubPullRequests.length > 0 ? <section aria-label="GitHub links" className="mt-6 border-t pt-4">
              <h3 className="mb-2 text-xs font-semibold text-muted-foreground">GitHub</h3>
              <div className="grid gap-1">
                {githubPullRequests.map((link) => <a key={link.url} href={link.url} target="_blank" rel="noopener noreferrer" className="flex min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-xs text-foreground transition-colors duration-150 ease-out hover:bg-muted">
                  <Link2 className="size-4 shrink-0 text-muted-foreground" />
                  <span className="truncate">{link.title}</span>
                  <span className="ml-auto shrink-0 text-muted-foreground">{link.state === 'paused' ? 'Paused PR' : link.state === 'merged' ? 'Merged PR' : link.state === 'closed' ? 'Closed PR' : 'Open PR'}</span>
                </a>)}
              </div>
            </section> : null}
            <SubIssuesSection
              parent={task}
              projects={projects}
              statuses={state.statuses}
              users={users}
              currentUserId={state.currentUserId}
              composing={composingSubIssue}
              onComposingChange={(open) => {
                if (!open) restoreSubIssueFocus.current = true
                setComposingSubIssue(open)
              }}
              addButtonRef={subIssueHeaderAddRef}
              onAddExisting={() => setParentPicker('add')}
              onOpen={openTask}
            />
            <TaskRelationsSection
              relations={relations}
              statuses={state.statuses}
              projects={projects}
              newRelationId={newRelationId}
              removingId={removeRelation.isPending ? removeRelation.variables : undefined}
              onOpen={openTask}
              onRemove={(relationId) => removeRelation.mutate(relationId)}
            />

          </div>

          <aside className="min-[900px]:col-start-2 min-[900px]:[grid-row:1/span_2] min-[900px]:border-l min-[900px]:px-5 min-[900px]:pt-8 min-[900px]:pb-6 max-[899px]:grid max-[899px]:w-full max-[899px]:grid-cols-2 max-[899px]:gap-x-3 max-[899px]:gap-y-3.5 max-[899px]:border-y max-[899px]:py-3.5">
            <PropertyGroup title="Properties" className="max-[899px]:col-span-full max-[899px]:flex-row max-[899px]:flex-wrap max-[899px]:items-center max-[899px]:gap-1 max-[899px]:*:data-[slot=property-heading]:w-full">
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <PropertyButton>
                      <TaskStatusIcon status={status} />
                      {status?.name ?? 'No status'}
                    </PropertyButton>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-45">
                  {statusOptions.map((option) => (
                    <DropdownMenuItem
                      key={option.id}
                      className="data-selected:bg-accent data-selected:font-medium"
                      data-selected={option.id === task.statusId || undefined}
                      onClick={() => option.category === 'duplicate'
                        ? setPicker('duplicate')
                        : updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })}
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
                    <PropertyButton>
                      <PriorityIcon priority={task.priority} />
                      {task.priority === 'none' ? 'Set priority' : PRIORITY_LABEL[task.priority]}
                    </PropertyButton>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-45">
                  {PRIORITY_ORDER.map((priority) => (
                    <DropdownMenuItem
                      key={priority}
                      className="data-selected:bg-accent data-selected:font-medium"
                      data-selected={priority === task.priority || undefined}
                      onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, priority } })}
                    >
                      <PriorityIcon priority={priority} />
                      {PRIORITY_LABEL[priority]}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
              {/* Multi-assignee options toggle individually; the checked indicator marks the active ones. */}
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <PropertyButton>
                      {assignees.length > 0 ? (
                        <>
                          <UserAvatarStack users={assignees} size={16} />
                          <span className="truncate">{assignees.map((u) => u.name).join(', ')}</span>
                        </>
                      ) : (
                        <>
                          <UserIcon aria-hidden="true" className="text-muted-foreground" />
                          Assign
                        </>
                      )}
                    </PropertyButton>
                  }
                />
                <DropdownMenuContent className="w-auto min-w-45">
                  <DropdownMenuGroup>
                    <DropdownMenuLabel>Assignees</DropdownMenuLabel>
                    {users.map((u) => {
                      const active = task.assigneeIds.includes(u.id)
                      return (
                        <DropdownMenuCheckboxItem
                          key={u.id}
                          checked={active}
                          closeOnClick
                          onCheckedChange={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, assignee_ids: active ? task.assigneeIds.filter((id) => id !== u.id) : [...task.assigneeIds, u.id] } })}
                        >
                          <UserAvatar user={u} size={16} />
                          {u.name}
                        </DropdownMenuCheckboxItem>
                      )
                    })}
                  </DropdownMenuGroup>
                </DropdownMenuContent>
              </DropdownMenu>
              {task.parent ? (
                <DropdownMenu>
                  <DropdownMenuTrigger render={
                    <PropertyButton className="max-w-full" aria-label={`Parent: ${refIdentifier(task.parent)} ${task.parent.title}`}>
                      <Hierarchy2 className="size-3.5" aria-hidden="true" />
                      <span className="shrink-0 text-muted-foreground tabular-nums">{refIdentifier(task.parent)}</span>
                      <span className="truncate">{task.parent.title || 'Untitled'}</span>
                    </PropertyButton>
                  } />
                  <DropdownMenuContent className="w-auto min-w-45">
                    <DropdownMenuItem onClick={() => openTask(task.parent!.id)}>Open parent</DropdownMenuItem>
                    <DropdownMenuItem onClick={() => setParentPicker('set')}>Change parent…</DropdownMenuItem>
                    <DropdownMenuItem onClick={() => void parentActions.setParent([task], null)}>Remove parent</DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : (
                <PropertyButton className="text-muted-foreground" onClick={() => setParentPicker('set')}>
                  <Hierarchy2 className="size-3.5" aria-hidden="true" />
                  Set parent
                </PropertyButton>
              )}
            </PropertyGroup>

            <PropertyGroup title="Labels">
              <TaskLabels workspaceId={workspace.id} labelIds={task.labels} labels={state.labels} onChange={(labelIds) => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, label_ids: labelIds } })} />
            </PropertyGroup>

            <PropertyGroup title="Project">
              {githubIssueLinked ? (
                // GitHub sync finds the task through its project's repository, so it cannot move
                <>
                  {project ? <LabelPill label={project} /> : <span className="text-xs text-muted-foreground/70">—</span>}
                  <p className="mt-1 text-xs text-muted-foreground">Synced with GitHub, so it stays in this project.</p>
                </>
              ) : (
                <DropdownMenu>
                  <DropdownMenuTrigger
                    render={
                      // the pill is the trigger, like "Add label": a ghost button around a badge reads as two controls
                      <Button variant="outline" size="xs" className="h-5 max-w-full rounded-full font-medium" aria-label={`Project: ${project?.name ?? 'none'}`}>
                        {project ? <ColorDot color={project.color} /> : null}
                        <span className="truncate">{project?.name ?? 'Set project'}</span>
                      </Button>
                    }
                  />
                  <DropdownMenuContent className="w-auto min-w-52">
                    <DropdownMenuGroup>
                      <DropdownMenuLabel>Move to project</DropdownMenuLabel>
                      {projects.map((item) => (
                        <DropdownMenuItem
                          key={item.id}
                          className="data-selected:bg-accent data-selected:font-medium"
                          data-selected={item.id === task.projectId || undefined}
                          onClick={() => moveToProject([task], item)}
                        >
                          <ColorDot color={item.color} className="size-2" />
                          <span className="flex-1 truncate">{item.name}</span>
                          <span className="text-xs text-muted-foreground tabular-nums">{item.key}</span>
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuGroup>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </PropertyGroup>

            <PropertyGroup title="Source">
              {task.sourceUrl ? (
                <a
                  href={task.sourceUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className={cn(buttonVariants({ variant: 'ghost' }), '-ml-2 max-w-full text-[13px] max-[899px]:h-7 max-[899px]:px-1.5 max-[899px]:text-xs')}
                >
                  <Link2 className="size-3.5" aria-hidden="true" />
                  <span className="truncate">{task.sourceUrl.replace(/^https?:\/\//, '').split('/')[0]}</span>
                </a>
              ) : null}
              <Popover open={sourceOpen} onOpenChange={setSourceOpen}>
                <PopoverTrigger render={<PropertyButton type="button" className="h-7 text-muted-foreground">{task.sourceUrl ? 'Edit source' : 'Add source'}</PropertyButton>} />
                <PopoverContent align="start" className="w-72 gap-2 p-3">
                  <form onSubmit={(event) => {
                    event.preventDefault()
                    updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, source_url: new FormData(event.currentTarget).get('source_url')?.toString().trim() || null } })
                    setSourceOpen(false)
                  }} className="flex flex-col gap-2">
                    <label htmlFor="task-source-url" className="text-xs font-medium">Source URL</label>
                    <Input id="task-source-url" name="source_url" type="url" placeholder="https://..." defaultValue={task.sourceUrl ?? ''} />
                    <Button type="submit" size="sm">Save</Button>
                  </form>
                </PopoverContent>
              </Popover>
            </PropertyGroup>

            <PropertyGroup title="Due date">
              <Popover open={dueDateOpen} onOpenChange={setDueDateOpen}>
                <PopoverTrigger
                  render={
                    <PropertyButton type="button" aria-label="Due date">
                      <Calendar className="size-3.5" aria-hidden="true" />
                      <span>{dueDateLabel(task.dueAt, task.dueStartAt)}</span>
                    </PropertyButton>
                  }
                />
                <PopoverContent align="start" side="top" className="w-auto gap-0 p-0">
                  <DatePicker
                    startValue={task.dueStartAt ?? null}
                    value={task.dueAt}
                    onClear={() => {
                      // through the same queue as the picks, so Clear right after a pick does not send a stale version
                      saveDueDate({ due_start_at: null, due_at: null })
                      setDueDateOpen(false)
                    }}
                    onChange={({ start, end }) => saveDueDate({ due_start_at: start, due_at: end })}
                  />
                </PopoverContent>
              </Popover>
            </PropertyGroup>
          </aside>

          <div className="col-start-1 min-w-0 min-[900px]:mx-auto min-[900px]:w-full min-[900px]:max-w-[840px] min-[900px]:px-10 min-[900px]:pb-12">
            <ActivityFeed task={task} state={state} onOpenTask={onOpenTask} />

            {/* the chat composer: markdown, @mentions, emoji, attachments (paste / drop / pick) */}
            <div className="mt-4 max-[899px]:mt-3">
              <TaskCommentComposer placeholder="Leave a comment…" pending={createComment.isPending} progress={createComment.progress} error={createComment.isError ? `${createComment.remainingCount || 'Comment'} upload failed.` : undefined} members={users} onSend={(body, files, mentionedUserIds) => createComment.mutateAsync({ body, files, mentionedUserIds })} />
            </div>
          </div>
          {picker ? (
            <TaskPickerDialog
              open
              onOpenChange={(open) => { if (!open) setPicker(null) }}
              title={pickerTitle(picker, task.identifier)}
              statuses={state.statuses}
              excludeIds={picker === 'duplicate' ? [task.id] : [task.id, ...relatedTaskIds(relations)]}
              excludeDuplicates={picker === 'duplicate'}
              onSelect={choose}
            />
          ) : null}
          {parentPicker ? (
            <TaskPickerDialog
              open
              onOpenChange={(open) => { if (!open) setParentPicker(null) }}
              title={parentPicker === 'set' ? parentPickerTitle(task.identifier) : `Add a sub-issue to ${task.identifier}…`}
              statuses={state.statuses}
              // set: never itself or anything below it that is loaded (deeper ones: the server's parent_cycle toast);
              // add: never itself, an ancestor, or a sub-issue it already has
              excludeIds={parentPicker === 'set'
                ? [task.id, ...cachedDescendantIds(queryClient, workspace.id, task.id)]
                : [task.id, ...(task.ancestors ?? []).map((ref) => ref.id), ...childIds]}
              onSelect={(target) => void (parentPicker === 'set'
                ? parentActions.setParent([task], { id: target.id, identifier: target.identifier })
                : parentActions.setParent([target], { id: task.id, identifier: task.identifier }))}
            />
          ) : null}
        </div>
      )}
    </Pane>
  )
}

/** A titled block of the properties sidebar; on phones the sidebar becomes a two-column grid of these. */
function PropertyGroup({ title, className, children }: { title: string; className?: string; children: ReactNode }) {
  return (
    <div data-slot="property-group" className={cn('mb-6 flex flex-col items-start gap-0.5 max-[899px]:mb-0 max-[899px]:min-w-0', className)}>
      <h4 data-slot="property-heading" className="mb-1.5 text-xs font-medium text-muted-foreground/70">{title}</h4>
      {children}
    </div>
  )
}

/** A sidebar property value that opens its editor: a ghost button pulled left so its text lines up with the heading. */
function PropertyButton({ className, ...props }: ComponentProps<typeof Button>) {
  return (
    <Button
      variant="ghost"
      className={cn('-ml-2 text-[13px] max-[899px]:h-7 max-[899px]:max-w-full max-[899px]:px-1.5 max-[899px]:text-xs', className)}
      {...props}
    />
  )
}
