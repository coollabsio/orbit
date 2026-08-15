import { useState } from 'react'
import { ArrowLeft, CloseCircle, TaskSquare } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { EmptyState } from '../../../components/ui/EmptyState'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import {
  PRIORITY_LABEL,
  PRIORITY_ORDER,
  STATUS_LABEL,
  STATUS_ORDER,
} from '../../../components/workspace/taskMeta'
import { fullDate } from '../../../lib/format'
import {
  addTaskComment,
  setTaskAssignee,
  setTaskDescription,
  setTaskPriority,
  setTaskStatus,
  setTaskTitle,
} from '../../../mock/actions'
import type { Project, Task, User } from '../../../mock/types'
import { ActivityFeed } from './ActivityFeed'

interface TaskDetailProps {
  task: Task | undefined
  project: Project | undefined
  users: User[]
  onBack: () => void
}

export function TaskDetail({ task, project, users, onBack }: TaskDetailProps) {
  const [comment, setComment] = useState('')
  const assignee = users.find((u) => u.id === task?.assigneeId)

  const submitComment = () => {
    const body = comment.trim()
    if (!body || !task) return
    addTaskComment(task.id, body)
    setComment('')
  }

  return (
    <section className="pane tasks-detail-pane">
      <div className="pane-header">
        <button className="icon-button tasks-back" onClick={onBack} aria-label="Back to tasks">
          <ArrowLeft size={16} />
        </button>
        <span className="text-faint text-xs">{task?.identifier ?? 'Task'}</span>
        <div className="spacer" />
        <button className="icon-button tasks-close" onClick={onBack} aria-label="Close detail">
          <CloseCircle size={16} />
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
        <>
          <div className="pane-body" style={{ padding: '16px 16px 8px' }}>
            <input
              key={task.id}
              className="tasks-detail-title"
              defaultValue={task.title}
              placeholder="Task title"
              aria-label="Task title"
              onBlur={(e) => {
                const value = e.target.value.trim()
                if (value && value !== task.title) setTaskTitle(task.id, value)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur()
              }}
            />

            <div className="tasks-props">
              <span className="tasks-prop-label">Status</span>
              <div className="tasks-prop-value">
                <Dropdown
                  trigger={() => (
                    <button className="button button-ghost">
                      <TaskStatusIcon status={task.status} />
                      {STATUS_LABEL[task.status]}
                    </button>
                  )}
                >
                  {(close) => (
                    <>
                      {STATUS_ORDER.map((status) => (
                        <button
                          key={status}
                          className="popover-option"
                          data-selected={status === task.status || undefined}
                          onClick={() => {
                            setTaskStatus(task.id, status)
                            close()
                          }}
                        >
                          <TaskStatusIcon status={status} />
                          {STATUS_LABEL[status]}
                        </button>
                      ))}
                    </>
                  )}
                </Dropdown>
              </div>

              <span className="tasks-prop-label">Priority</span>
              <div className="tasks-prop-value">
                <Dropdown
                  trigger={() => (
                    <button className="button button-ghost">
                      <PriorityIcon priority={task.priority} />
                      {PRIORITY_LABEL[task.priority]}
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
                            setTaskPriority(task.id, priority)
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
              </div>

              <span className="tasks-prop-label">Assignee</span>
              <div className="tasks-prop-value">
                <Dropdown
                  trigger={() => (
                    <button className="button button-ghost">
                      {assignee ? (
                        <>
                          <Avatar user={assignee} size={16} />
                          {assignee.name}
                        </>
                      ) : (
                        'Unassigned'
                      )}
                    </button>
                  )}
                >
                  {(close) => (
                    <>
                      <button
                        className="popover-option"
                        data-selected={!task.assigneeId || undefined}
                        onClick={() => {
                          setTaskAssignee(task.id, null)
                          close()
                        }}
                      >
                        Unassigned
                      </button>
                      {users.map((u) => (
                        <button
                          key={u.id}
                          className="popover-option"
                          data-selected={u.id === task.assigneeId || undefined}
                          onClick={() => {
                            setTaskAssignee(task.id, u.id)
                            close()
                          }}
                        >
                          <Avatar user={u} size={16} />
                          {u.name}
                        </button>
                      ))}
                    </>
                  )}
                </Dropdown>
              </div>

              <span className="tasks-prop-label">Project</span>
              <div className="tasks-prop-value">
                {project ? (
                  <span className="pill">
                    <span className="pill-dot" style={{ background: project.color }} />
                    {project.name}
                  </span>
                ) : (
                  <span className="text-faint">—</span>
                )}
              </div>

              <span className="tasks-prop-label">Due date</span>
              <div className="tasks-prop-value">
                <span className={task.dueAt ? undefined : 'text-faint'} style={{ fontSize: 13 }}>
                  {task.dueAt ? fullDate(task.dueAt) : '—'}
                </span>
              </div>

              <span className="tasks-prop-label">Labels</span>
              <div className="tasks-prop-value">
                {task.labels.length > 0 ? (
                  task.labels.map((label) => (
                    <span key={label} className="pill">
                      {label}
                    </span>
                  ))
                ) : (
                  <span className="text-faint">—</span>
                )}
              </div>
            </div>

            <textarea
              key={`desc-${task.id}`}
              className="tasks-desc"
              defaultValue={task.description}
              placeholder="Add a description…"
              aria-label="Description"
              onBlur={(e) => {
                if (e.target.value !== task.description) setTaskDescription(task.id, e.target.value)
              }}
            />

            <ActivityFeed task={task} users={users} />
          </div>

          <div className="tasks-composer">
            <input
              className="input"
              placeholder="Leave a comment…"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submitComment()
              }}
            />
            <button className="button" onClick={submitComment} disabled={!comment.trim()}>
              Comment
            </button>
          </div>
        </>
      )}
    </section>
  )
}
