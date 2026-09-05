import { useMemo, useRef, useState } from 'react'
import { Add, ArrowLeft, Calendar, Paperclip2, TaskSquare, Trash, Xmark } from 'reicon-react'
import { Avatar, AvatarStack } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { EmptyState } from '../../../components/ui/EmptyState'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER, projectStatuses } from '../../../components/workspace/taskMeta'
import type { Project, Task, TaskViewState } from '../api/models'
import { useCreateTaskComment, useDeleteTask, useDeleteTaskAttachment, useUpdateTask, useUploadTaskAttachments } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { clipboardFiles } from '../../chat/attachmentLib'
import { Attachments } from '../../chat/components/Attachments'
import { ActivityFeed } from './ActivityFeed'
import { TaskCommentComposer } from './TaskCommentComposer'

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
  const [dropOver, setDropOver] = useState(false)
  const attach = (files: FileList | File[] | null) => {
    if (task && files && files.length > 0) uploadAttachments.mutate(Array.from(files))
  }
  const status = state.statuses.find((s) => s.id === task?.statusId)
  const statusOptions = task ? projectStatuses(state.statuses, task.projectId) : []
  const assignees = users.filter((u) => task?.assigneeIds.includes(u.id))
  // every label used in the workspace, so a task can pick from the existing ones
  const allLabels = useMemo(
    () => Array.from(new Set(state.tasks.flatMap((t) => t.labels))).sort((a, b) => a.localeCompare(b)),
    [state.tasks],
  )

  return (
    <section className="pane tasks-detail-pane">
      <div className="pane-header">
        <button className="icon-button" onClick={onBack} aria-label="Back to tasks">
          <ArrowLeft size={16} />
        </button>
        <span className="text-faint text-xs">{task?.identifier ?? 'Task'}</span>
        <div className="spacer" />
        {task ? <button className="icon-button" aria-label="Delete task" title="Move to trash" onClick={() => {
          if (window.confirm(`Move ${task.identifier} to trash?`)) void deleteTask.mutateAsync({ taskId: task.id, version: task.version }).then(onBack)
        }}><Trash size={15} /></button> : null}
        <button className="icon-button" onClick={onBack} aria-label="Close task">
          <Xmark size={16} />
        </button>
      </div>
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
            <input
              key={task.id}
              className="tasks-detail-title"
              defaultValue={task.title}
              placeholder="Task title"
              aria-label="Task title"
              autoFocus={task.title === 'Untitled'}
              onBlur={(e) => {
                const value = e.target.value.trim()
                if (value && value !== task.title) updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, title: value } })
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur()
              }}
            />

            {/* description: paste (screenshots, files), drop, or pick attachments; they list below the text */}
            <div
              className="tasks-desc-wrap"
              data-drop-over={dropOver || undefined}
              onDragOver={(e) => {
                if (!e.dataTransfer.types.includes('Files')) return
                e.preventDefault()
                if (!dropOver) setDropOver(true)
              }}
              onDragLeave={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropOver(false)
              }}
              onDrop={(e) => {
                if (!e.dataTransfer.types.includes('Files')) return
                e.preventDefault()
                setDropOver(false)
                attach(e.dataTransfer.files)
              }}
            >
              <textarea
                key={`desc-${task.id}`}
                className="tasks-desc"
                defaultValue={task.description}
                placeholder="Add description… (paste or drop images and files)"
                aria-label="Description"
                onBlur={(e) => {
                  if (e.target.value !== task.description) updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, description: e.target.value } })
                }}
                onPaste={(e) => {
                  const files = clipboardFiles(e)
                  if (files.length === 0) return
                  e.preventDefault()
                  attach(files)
                }}
              />
              {task.attachments.length > 0 ? (
                <Attachments attachments={task.attachments} onRemove={(id) => deleteAttachment.mutate(id)} />
              ) : null}
              <div className="tasks-desc-tools">
                <input ref={fileInputRef} type="file" multiple hidden aria-label="Attach files" onChange={(e) => attach(e.target.files)} />
                <button type="button" className="button button-ghost tasks-attach" onClick={() => fileInputRef.current?.click()}>
                  <Paperclip2 size={14} />
                  Attach
                </button>
                {uploadAttachments.isPending ? <span className="text-faint text-xs">Uploading {uploadAttachments.progress}%</span> : null}
              </div>
            </div>

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
              {/* multi-assignee: options toggle and stay open; active ones show an × at the end */}
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
                {() => (
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
                          onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, assignee_ids: active ? task.assigneeIds.filter((id) => id !== u.id) : [...task.assigneeIds, u.id] } })}
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
              <div className="tasks-side-pills">
                {task.labels.map((label) => (
                  <span key={label} className="pill tasks-label-pill">
                    {label}
                    <button
                      type="button"
                      className="tasks-label-remove"
                      aria-label={`Remove label ${label}`}
                      title="Remove label"
                      onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, label_ids: task.labels.filter((item) => item !== label) } })}
                    >
                      <Xmark size={12} />
                    </button>
                  </span>
                ))}
                {/* add / remove labels: existing labels toggle (× on active) */}
                <Dropdown
                  trigger={() => (
                    <button className="pill tasks-label-add" aria-label="Add label">
                      <Add size={12} />
                      {task.labels.length === 0 ? 'Add label' : null}
                    </button>
                  )}
                >
                  {() => (
                    <>
                      <div className="popover-heading">Labels</div>
                      {allLabels.map((label) => {
                        const active = task.labels.includes(label)
                        return (
                          <button
                            key={label}
                            className="popover-option"
                            data-selected={active || undefined}
                            aria-pressed={active}
                            onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, label_ids: active ? task.labels.filter((item) => item !== label) : [...task.labels, label] } })}
                          >
                            <span className="pill">{label}</span>
                            {active ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                          </button>
                        )
                      })}
                    </>
                  )}
                </Dropdown>
              </div>
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

            <div className="tasks-side-group">
              <h4 className="tasks-side-heading">Due date</h4>
              <button className="button button-ghost tasks-side-prop" disabled title="Due dates are not available in the server contract."><Calendar size={15} />Set due date</button>
            </div>
          </aside>

          <div className="tasks-detail-activity">
            <ActivityFeed task={task} state={state} />

            {/* the chat composer: markdown, @mentions, emoji, attachments (paste / drop / pick) */}
            <div className="tasks-comment-composer">
              <TaskCommentComposer placeholder="Leave a comment…" pending={createComment.isPending} onSend={(body, files) => createComment.mutateAsync({ body, files })} />
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
