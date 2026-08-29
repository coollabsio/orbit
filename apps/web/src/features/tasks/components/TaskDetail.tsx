import { useState } from 'react'
import { ArrowLeft, Calendar, TaskSquare, Xmark } from 'reicon-react'
import { Avatar, AvatarStack } from '../../../components/ui/Avatar'
import { DatePicker } from '../../../components/ui/DatePicker'
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
import { fullDate, timeOfDay } from '../../../lib/format'
import {
  addTaskComment,
  setTaskDescription,
  setTaskDueAt,
  setTaskPriority,
  setTaskStatus,
  setTaskTitle,
  toggleTaskAssignee,
} from '../../../mock/actions'
import type { Project, Task, User } from '../../../mock/types'
import { ActivityFeed } from './ActivityFeed'

interface TaskDetailProps {
  task: Task | undefined
  project: Project | undefined
  users: User[]
  onBack: () => void
}

/** Full-page task view: main column (title, description, activity, comment box) + properties column. */
export function TaskDetail({ task, project, users, onBack }: TaskDetailProps) {
  const [comment, setComment] = useState('')
  const assignees = users.filter((u) => task?.assigneeIds.includes(u.id))

  const submitComment = () => {
    const body = comment.trim()
    if (!body || !task) return
    addTaskComment(task.id, body)
    setComment('')
  }

  return (
    <section className="pane tasks-detail-pane">
      <div className="pane-header">
        <button className="icon-button" onClick={onBack} aria-label="Back to tasks">
          <ArrowLeft size={16} />
        </button>
        <span className="text-faint text-xs">{task?.identifier ?? 'Task'}</span>
        <div className="spacer" />
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
              onBlur={(e) => {
                const value = e.target.value.trim()
                if (value && value !== task.title) setTaskTitle(task.id, value)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur()
              }}
            />

            <textarea
              key={`desc-${task.id}`}
              className="tasks-desc"
              defaultValue={task.description}
              placeholder="Add description…"
              aria-label="Description"
              onBlur={(e) => {
                if (e.target.value !== task.description) setTaskDescription(task.id, e.target.value)
              }}
            />

            <ActivityFeed task={task} users={users} />

            <div className="tasks-comment-box">
              <textarea
                className="tasks-comment-input"
                placeholder="Leave a comment…"
                aria-label="Comment"
                value={comment}
                onChange={(e) => setComment(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submitComment()
                }}
              />
              <div className="tasks-comment-actions">
                <button className="button button-primary" onClick={submitComment} disabled={!comment.trim()}>
                  Comment
                </button>
              </div>
            </div>
          </div>

          <aside className="tasks-detail-side">
            <div className="tasks-side-group">
              <h4 className="tasks-side-heading">Properties</h4>
              <Dropdown
                trigger={() => (
                  <button className="button button-ghost tasks-side-prop">
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
                          onClick={() => toggleTaskAssignee(task.id, u.id)}
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
                {task.labels.length > 0 ? (
                  task.labels.map((label) => (
                    <span key={label} className="pill">
                      {label}
                    </span>
                  ))
                ) : (
                  <span className="text-faint text-xs">No labels</span>
                )}
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
              <Dropdown
                className="tasks-date-dropdown"
                trigger={() => (
                  <button className="button button-ghost tasks-side-prop">
                    <Calendar size={15} />
                    {task.dueAt ? `${fullDate(task.dueAt)} · ${timeOfDay(task.dueAt)}` : 'Set due date'}
                  </button>
                )}
              >
                {(close) => (
                  <DatePicker
                    value={task.dueAt}
                    onChange={(iso) => setTaskDueAt(task.id, iso)}
                    onClear={() => {
                      setTaskDueAt(task.id, null)
                      close()
                    }}
                    onDone={close}
                  />
                )}
              </Dropdown>
            </div>
          </aside>
        </div>
      )}
    </section>
  )
}
