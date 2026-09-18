import { useRef } from 'react'
import { confirmAction } from '../../../components/ui/confirmAction'
import { Calendar, MoreH, Paperclip2, TaskSquare, Xmark } from 'reicon-react'
import { Avatar, AvatarStack } from '../../../components/ui/Avatar'
import { DatePicker } from '../../../components/ui/DatePicker'
import { Dropdown } from '../../../components/ui/Dropdown'
import { EmptyState } from '../../../components/ui/EmptyState'
import { TopbarBreadcrumb } from '../../../components/shell/TopbarBreadcrumb'
import { TopbarSlot } from '../../../components/shell/TopbarSlot'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses } from '../../../components/workspace/taskMeta'
import { taskFromRecord, type Project, type Task, type TaskViewState } from '../api/models'
import { useCreateTaskComment, useDeleteTask, useDeleteTaskAttachment, useTask, useUpdateTask, useUploadTaskAttachments } from '../api/tasks'
import { taskCrumbs } from '../taskCrumbs'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { Attachments } from '../../chat/components/Attachments'
import { ActivityFeed } from './ActivityFeed'
import { ReferencedBy } from './ReferencedBy'
import { SubIssues } from './SubIssues'
import { DuplicateBanner, DuplicateErrors, DuplicateMenuItems, DuplicatesGroup, MarkDuplicateDialog, useDuplicateFlow } from './TaskDuplicates'
import { TaskCommentComposer } from './TaskCommentComposer'
import { TaskLabels } from './TaskLabels'
import { TaskTextFields } from './TaskTextFields'

function dueDateLabel(value: string | null) {
  if (!value) return 'Set due date'
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
  /** Every workspace project: sub-issues may live in another project. */
  projects: Project[]
  state: TaskViewState
  onBack: () => void
  /** Navigate to another task (sub-issues, duplicates, backlinks). */
  onOpenTask: (taskId: string) => void
}

/** Full-page task view: main column (title, description, activity, comment composer) + properties column. */
export function TaskDetail({ task, project, projects, state, onBack, onOpenTask }: TaskDetailProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const uploadAttachments = useUploadTaskAttachments(workspace.id, task?.id ?? '')
  const deleteAttachment = useDeleteTaskAttachment(workspace.id, task?.id ?? '')
  const createComment = useCreateTaskComment(workspace.id, task?.id ?? '')
  const deleteTask = useDeleteTask(workspace.id)
  const duplicateFlow = useDuplicateFlow(task)
  // The parent is usually on the loaded list page; a deeper sub-issue's parent
  // may not be (the list hides sub-issues), so fetch just that one on demand.
  // A trashed parent answers 404, which the crumb renders as "In trash".
  const parentOnPage = task?.parentId ? state.tasks.find((candidate) => candidate.id === task.parentId) : undefined
  const parentQuery = useTask(workspace.id, task?.parentId && !parentOnPage ? task.parentId : undefined)
  const knownTasks = parentQuery.data ? [...state.tasks, taskFromRecord(parentQuery.data)] : state.tasks
  const users = state.users
  const fileInputRef = useRef<HTMLInputElement>(null)
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
    <section className="pane tasks-detail-pane">
      <TopbarSlot side="left">
        <TopbarBreadcrumb crumbs={task ? taskCrumbs(task, knownTasks, parentQuery.isLoading) : [{ label: 'Task' }]} />
        {task ? (
          <span className="topbar-status">
            <TaskStatusIcon status={status} size={12} />
            {status?.name ?? 'No status'}
          </span>
        ) : null}
      </TopbarSlot>
      <TopbarSlot side="right">
        {task ? (
          <Dropdown align="right" trigger={(open) => (
            <button type="button" className="icon-button" data-open={open || undefined} aria-label="More task actions">
              <MoreH size={16} />
            </button>
          )}>
            {(close) => (
              <>
                <DuplicateMenuItems task={task} flow={duplicateFlow} close={close} />
                <div className="popover-separator" />
                <button type="button" className="popover-option" data-tone="danger" disabled={deleteTask.isPending} onClick={async () => {
                  close()
                  if (!await confirmAction({ title: `Move ${task.identifier} to trash?`, description: 'You can restore this task from trash later.', confirmLabel: 'Move to trash', danger: true })) return
                  void deleteAndClose({ taskId: task.id, version: task.version })
                }}>Delete</button>
              </>
            )}
          </Dropdown>
        ) : null}
        <button className="icon-button" onClick={onBack} aria-label="Close task">
          <Xmark size={16} />
        </button>
      </TopbarSlot>
      {!task ? (
        <div className="pane-body">
          <EmptyState
            icon={TaskSquare}
            title="Task not found"
            description="This task does not exist or was removed."
          />
        </div>
      ) : (
        <div className="pane-body tasks-detail-body">
          <div className="tasks-detail-main">
            <DuplicateBanner task={task} tasks={state.tasks} />
            <TaskTextFields
              task={task}
              workspaceId={workspace.id}
              members={users}
              statuses={state.statuses}
              onUpdate={(update) => updateTask.mutateAsync({ taskId: task.id, body: { expected_version: task.version, ...update } })}
              onAttachFiles={attach}
            >
              {task.attachments.length > 0 ? (
                <Attachments attachments={task.attachments} onRemove={(id) => deleteAttachment.mutate(id)} />
              ) : null}
              <div className="tasks-desc-tools">
                <input ref={fileInputRef} type="file" multiple hidden aria-label="Attach files" onChange={(e) => attach(e.target.files)} />
                <button type="button" className="button button-ghost tasks-attach" onClick={() => fileInputRef.current?.click()}>
                  <Paperclip2 size={14} />
                  Attach
                </button>
                {uploadAttachments.isPending ? <span role="status" aria-live="polite" className="text-faint text-xs">Uploading {uploadAttachments.progress}%</span> : null}
                {uploadAttachments.isError ? <span role="alert" className="text-danger text-xs">{uploadAttachments.remainingCount} file(s) remain. <button type="button" className="button button-ghost" onClick={uploadAttachments.retry}>Retry upload</button></span> : null}
              </div>
            </TaskTextFields>
            {updateTask.isError ? <p role="alert" className="text-danger text-xs">Task update failed. <button type="button" className="button button-ghost" onClick={() => updateTask.variables && updateTask.mutate(updateTask.variables)}>Retry</button></p> : null}
            {deleteAttachment.isError ? <p role="alert" className="text-danger text-xs">Attachment removal failed. <button type="button" className="button button-ghost" onClick={() => deleteAttachment.variables && deleteAttachment.mutate(deleteAttachment.variables)}>Retry</button></p> : null}
            {deleteTask.isError ? <p role="alert" className="text-danger text-xs">Task deletion failed. <button type="button" className="button button-ghost" onClick={() => deleteTask.variables && void deleteAndClose(deleteTask.variables)}>Retry</button></p> : null}
            <DuplicateErrors flow={duplicateFlow} />
            <SubIssues task={task} projects={projects} state={state} onOpen={onOpenTask} />
            <MarkDuplicateDialog task={task} flow={duplicateFlow} workspaceId={workspace.id} />

          </div>

          <aside className="tasks-detail-side">
            <div className="tasks-side-group">
              <h4 className="tasks-side-heading">Properties</h4>
              <Dropdown
                trigger={() => (
                  <button className="button button-ghost tasks-side-prop">
                    <TaskStatusIcon status={status} />
                    {status?.name ?? 'No status'}
                  </button>
                )}
              >
                {(close) => (
                  <>
                    {statusOptions.map((option) => (
                      <button
                        key={option.id}
                        className="popover-option"
                        data-selected={option.id === task.statusId || undefined}
                        onClick={() => {
                          updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })
                          close()
                        }}
                      >
                        <TaskStatusIcon status={option} />
                        {option.name}
                      </button>
                    ))}
                  </>
                )}
              </Dropdown>
              <Dropdown
                trigger={() => (
                  <button className="button button-ghost tasks-side-prop">
                    <PriorityIcon priority={task.priority} />
                    {task.priority === 'none' ? 'Set priority' : PRIORITY_LABEL[task.priority]}
                  </button>
                )}
              >
                {(close) => (
                  <>
                    {PRIORITY_ORDER.map((priority) => (
                      <button
                        key={priority}
                        className="popover-option"
                        data-selected={priority === task.priority || undefined}
                        onClick={() => {
                          updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, priority } })
                          close()
                        }}
                      >
                        <PriorityIcon priority={priority} />
                        {PRIORITY_LABEL[priority]}
                      </button>
                    ))}
                  </>
                )}
              </Dropdown>
              {/* Multi-assignee options toggle individually; active ones show an × at the end. */}
              <Dropdown
                trigger={() => (
                  <button className="button button-ghost tasks-side-prop">
                    {assignees.length > 0 ? (
                      <>
                        <AvatarStack users={assignees} size={16} />
                        <span className="truncate">{assignees.map((u) => u.name).join(', ')}</span>
                      </>
                    ) : (
                      <>
                        <Avatar user={undefined} size={16} name="—" />
                        Assign
                      </>
                    )}
                  </button>
                )}
              >
                {(close) => (
                  <>
                    <div className="popover-heading">Assignees</div>
                    {users.map((u) => {
                      const active = task.assigneeIds.includes(u.id)
                      return (
                        <button
                          key={u.id}
                          className="popover-option"
                          data-selected={active || undefined}
                          aria-pressed={active}
                          onClick={() => {
                            updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, assignee_ids: active ? task.assigneeIds.filter((id) => id !== u.id) : [...task.assigneeIds, u.id] } })
                            close()
                          }}
                        >
                          <Avatar user={u} size={16} />
                          {u.name}
                          {active ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                        </button>
                      )
                    })}
                  </>
                )}
              </Dropdown>
            </div>

            <div className="tasks-side-group">
              <h4 className="tasks-side-heading">Labels</h4>
              <TaskLabels workspaceId={workspace.id} labelIds={task.labels} labels={state.labels} onChange={(labelIds) => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, label_ids: labelIds } })} />
            </div>

            <div className="tasks-side-group">
              <h4 className="tasks-side-heading">Project</h4>
              {project ? (
                <span className="pill">
                  <span className="pill-dot" style={{ background: project.color }} />
                  {project.name}
                </span>
              ) : (
                <span className="text-faint text-xs">—</span>
              )}
            </div>

            <DuplicatesGroup task={task} tasks={state.tasks} onOpen={onOpenTask} />
            <ReferencedBy task={task} onOpen={onOpenTask} />

            <div className="tasks-side-group">
              <h4 className="tasks-side-heading">Due date</h4>
              <Dropdown
                className="tasks-date-dropdown"
                direction="up"
                trigger={(open) => (
                  <button
                    type="button"
                    className="tasks-due-date"
                    aria-label="Due date"
                    aria-expanded={open}
                    disabled={updateTask.isPending}
                  >
                    <Calendar size={15} aria-hidden="true" />
                    <span>{dueDateLabel(task.dueAt)}</span>
                  </button>
                )}
              >
                {(close) => (
                  <DatePicker
                    value={task.dueAt}
                    onChange={(dueAt) => updateTask.mutate({
                      taskId: task.id,
                      body: { expected_version: task.version, due_at: dueAt },
                    })}
                    onClear={() => {
                      updateTask.mutate({
                        taskId: task.id,
                        body: { expected_version: task.version, due_at: null },
                      })
                      close()
                    }}
                    onDone={close}
                  />
                )}
              </Dropdown>
            </div>
          </aside>

          <div className="tasks-detail-activity">
            <ActivityFeed task={task} state={state} />

            {/* rich text composer: formatting, unified @mentions, attachments (paste / drop / pick) */}
            <div className="tasks-comment-composer">
              <TaskCommentComposer
                placeholder="Leave a comment…"
                pending={createComment.isPending}
                progress={createComment.progress}
                error={createComment.isError ? `${createComment.remainingCount || 'Comment'} upload failed.` : undefined}
                workspaceId={workspace.id}
                members={users}
                statuses={state.statuses}
                onSend={(bodyJson, files) => createComment.mutateAsync({ bodyJson, files })}
              />
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
