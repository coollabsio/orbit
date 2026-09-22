import { useRef, useState } from 'react'
import { confirmAction } from '@/components/common/confirmAction'
import { ArrowLeft, Calendar, Paperclip2 as Paperclip, TaskSquare as SquareCheck, Xmark as X } from 'reicon-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import { DatePicker } from '@/components/common/DatePicker'
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
import { PriorityIcon } from './PriorityIcon'
import { TaskStatusIcon } from './TaskStatusIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses } from '@/features/tasks/taskMeta'
import type { Project, Task, TaskViewState } from '@/features/tasks/api/models'
import { useCreateTaskComment, useDeleteTask, useDeleteTaskAttachment, useUpdateTask, useUploadTaskAttachments } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { Attachments } from '@/components/common/Attachments'
import { ActivityFeed } from './ActivityFeed'
import { TaskCommentComposer } from './TaskCommentComposer'
import { TaskLabels } from './TaskLabels'
import { TaskTextFields } from './TaskTextFields'

const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`
/** Multi-select rows keep room on the right for the checked indicator. */
const CHECK_OPTION =
  `group min-h-8 cursor-pointer gap-2 py-1.5 pr-8 pl-2 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-checked:bg-accent data-checked:font-medium`
const HEADING = 'px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase'
const PILL = 'inline-flex h-[22px] items-center gap-1.5 overflow-visible rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const SIDE_PROP = '-ml-2 text-[13px] max-[899px]:h-7 max-[899px]:min-h-7 max-[899px]:max-w-full max-[899px]:px-1.5 max-[899px]:text-xs'
const SIDE_GROUP = 'mb-6 flex flex-col items-start gap-0.5 max-[899px]:mb-0 max-[899px]:min-w-0'
const SIDE_HEADING = 'mb-1.5 text-xs font-medium text-muted-foreground/70'

function dueDateLabel(value: string | null, startValue?: string | null) {
  if (!value) return 'Set due date'
  if (startValue) {
    const format = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
    return `${format.format(new Date(startValue))} – ${format.format(new Date(value))}`
  }
  return new Date(value).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

interface TaskDetailProps {
  task: Task | undefined
  project: Project | undefined
  state: TaskViewState
  onBack: () => void
}

/** Full-page task view: main column (title, description, activity, comment composer) + properties column. */
export function TaskDetail({ task, project, state, onBack }: TaskDetailProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const uploadAttachments = useUploadTaskAttachments(workspace.id, task?.id ?? '')
  const deleteAttachment = useDeleteTaskAttachment(workspace.id, task?.id ?? '')
  const createComment = useCreateTaskComment(workspace.id, task?.id ?? '')
  const deleteTask = useDeleteTask(workspace.id)
  const users = state.users
  const fileInputRef = useRef<HTMLInputElement>(null)
  // the date panel needs to close itself from Clear/Done, so the popover stays controlled
  const [dueDateOpen, setDueDateOpen] = useState(false)
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

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2 max-[899px]:border-b-0">
        <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={onBack} aria-label="Back to tasks">
          <ArrowLeft className="size-4" />
        </Button>
        <span className="text-xs text-muted-foreground/70">{task?.identifier ?? 'Task'}</span>
        <div className="flex-1" />
        {task ? <Button variant="destructive" title="Move to trash" disabled={deleteTask.isPending} onClick={async () => {
          if (!await confirmAction({ title: `Move ${task.identifier} to trash?`, description: 'You can restore this task from trash later.', confirmLabel: 'Move to trash', danger: true })) return
          void deleteAndClose({ taskId: task.id, version: task.version })
        }}>Delete</Button> : null}
        <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={onBack} aria-label="Close task">
          <X className="size-4" />
        </Button>
      </div>
      {!task ? (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <EmptyState
            icon={SquareCheck}
            title="Task not found"
            description="This task does not exist or was removed."
          />
        </div>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,760px)_240px] content-start items-start gap-x-12 overflow-y-auto px-10 pt-8 pb-12 max-[899px]:grid-cols-1 max-[899px]:gap-[14px] max-[899px]:px-3.5 max-[899px]:pt-4 max-[899px]:pb-7">
          <div className="min-w-0 max-w-[760px]">
            <TaskTextFields
              task={task}
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
                {uploadAttachments.isPending ? <span role="status" aria-live="polite" className="text-xs text-muted-foreground/70">Uploading {uploadAttachments.progress}%</span> : null}
                {uploadAttachments.isError ? <span role="alert" className="text-xs text-destructive">{uploadAttachments.remainingCount} file(s) remain. <Button variant="ghost" onClick={uploadAttachments.retry}>Retry upload</Button></span> : null}
              </div>
            </TaskTextFields>
            {updateTask.isError ? <p role="alert" className="text-xs text-destructive">Task update failed. <Button variant="ghost" onClick={() => updateTask.variables && updateTask.mutate(updateTask.variables)}>Retry</Button></p> : null}
            {deleteAttachment.isError ? <p role="alert" className="text-xs text-destructive">Attachment removal failed. <Button variant="ghost" onClick={() => deleteAttachment.variables && deleteAttachment.mutate(deleteAttachment.variables)}>Retry</Button></p> : null}
            {deleteTask.isError ? <p role="alert" className="text-xs text-destructive">Task deletion failed. <Button variant="ghost" onClick={() => deleteTask.variables && void deleteAndClose(deleteTask.variables)}>Retry</Button></p> : null}

          </div>

          <aside className="col-start-2 w-60 shrink-0 pt-1.5 [grid-row:1/span_2] max-[899px]:col-start-1 max-[899px]:row-auto max-[899px]:grid max-[899px]:w-full max-[899px]:grid-cols-2 max-[899px]:gap-x-3 max-[899px]:gap-y-3.5 max-[899px]:border-y max-[899px]:border-border max-[899px]:py-3.5">
            <div className={`${SIDE_GROUP} max-[899px]:col-span-full max-[899px]:flex-row max-[899px]:flex-wrap max-[899px]:items-center max-[899px]:gap-1`}>
              <h4 className={`${SIDE_HEADING} max-[899px]:w-full`}>Properties</h4>
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button variant="ghost" className={SIDE_PROP}>
                      <TaskStatusIcon status={status} />
                      {status?.name ?? 'No status'}
                    </Button>
                  }
                />
                <DropdownMenuContent className={MENU}>
                  {statusOptions.map((option) => (
                    <DropdownMenuItem
                      key={option.id}
                      className={OPTION}
                      data-selected={option.id === task.statusId || undefined}
                      onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })}
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
                    <Button variant="ghost" className={SIDE_PROP}>
                      <PriorityIcon priority={task.priority} />
                      {task.priority === 'none' ? 'Set priority' : PRIORITY_LABEL[task.priority]}
                    </Button>
                  }
                />
                <DropdownMenuContent className={MENU}>
                  {PRIORITY_ORDER.map((priority) => (
                    <DropdownMenuItem
                      key={priority}
                      className={OPTION}
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
                    <Button variant="ghost" className={SIDE_PROP}>
                      {assignees.length > 0 ? (
                        <>
                          <UserAvatarStack users={assignees} size={16} />
                          <span className="truncate">{assignees.map((u) => u.name).join(', ')}</span>
                        </>
                      ) : (
                        <>
                          <UserAvatar user={undefined} size={16} name="—" />
                          Assign
                        </>
                      )}
                    </Button>
                  }
                />
                <DropdownMenuContent className={MENU}>
                  <DropdownMenuGroup className="flex flex-col gap-px">
                    <DropdownMenuLabel className={HEADING}>Assignees</DropdownMenuLabel>
                    {users.map((u) => {
                      const active = task.assigneeIds.includes(u.id)
                      return (
                        <DropdownMenuCheckboxItem
                          key={u.id}
                          className={CHECK_OPTION}
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
            </div>

            <div className={SIDE_GROUP}>
              <h4 className={SIDE_HEADING}>Labels</h4>
              <TaskLabels workspaceId={workspace.id} labelIds={task.labels} labels={state.labels} onChange={(labelIds) => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, label_ids: labelIds } })} />
            </div>

            <div className={SIDE_GROUP}>
              <h4 className={SIDE_HEADING}>Project</h4>
              {project ? (
                <Badge variant="outline" className={PILL}>
                  <span className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />
                  {project.name}
                </Badge>
              ) : (
                <span className="text-xs text-muted-foreground/70">—</span>
              )}
            </div>

            <div className={SIDE_GROUP}>
              <h4 className={SIDE_HEADING}>Due date</h4>
              <Popover open={dueDateOpen} onOpenChange={setDueDateOpen}>
                <PopoverTrigger
                  render={
                    <Button
                      type="button"
                      variant="ghost"
                      className="-ml-2 inline-flex h-auto min-h-8 items-center gap-2 rounded-md border-0 px-2 py-1.5 text-[13px] font-normal text-foreground transition-colors hover:bg-accent aria-expanded:bg-accent dark:hover:bg-accent"
                      aria-label="Due date"
                      disabled={updateTask.isPending}
                    >
                      <Calendar className="size-3.5" aria-hidden="true" />
                      <span>{dueDateLabel(task.dueAt, task.dueStartAt)}</span>
                    </Button>
                  }
                />
                <PopoverContent align="start" side="top" className="w-auto gap-0 p-0">
                  <DatePicker
                    startValue={task.dueStartAt ?? null}
                    value={task.dueAt}
                    onClear={() => {
                      updateTask.mutate({
                        taskId: task.id,
                        body: { expected_version: task.version, due_start_at: null, due_at: null },
                      })
                      setDueDateOpen(false)
                    }}
                    onDone={({ start, end }) => {
                      updateTask.mutate({
                        taskId: task.id,
                        body: { expected_version: task.version, due_start_at: start, due_at: end },
                      })
                      setDueDateOpen(false)
                    }}
                  />
                </PopoverContent>
              </Popover>
            </div>
          </aside>

          <div className="col-start-1 min-w-0 max-w-[760px] max-[899px]:w-full max-[899px]:max-w-none">
            <ActivityFeed task={task} state={state} />

            {/* the chat composer: markdown, @mentions, emoji, attachments (paste / drop / pick) */}
            <div className="mt-4 max-[899px]:mt-3">
              <TaskCommentComposer placeholder="Leave a comment…" pending={createComment.isPending} progress={createComment.progress} error={createComment.isError ? `${createComment.remainingCount || 'Comment'} upload failed.` : undefined} members={users} onSend={(body, files, mentionedUserIds) => createComment.mutateAsync({ body, files, mentionedUserIds })} />
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
