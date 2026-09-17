import { Xmark } from 'reicon-react'
import { Avatar, AvatarStack } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { projectStatuses } from '../../../components/workspace/taskMeta'
import { shortDate } from '../../../lib/format'
import type { Task, TaskStatusDef, User } from '../api/models'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { useUpdateTask } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { PriorityPicker } from './PriorityPicker'
import { LinkifiedText } from './LinkifiedText'

interface TaskRowProps {
  task: Task
  statuses: TaskStatusDef[]
  labels: LabelRecord[]
  users: User[]
  assignees: User[]
  selected: boolean
  dragging: boolean
  onOpen: (taskId: string) => void
  onToggleSelect: (taskId: string) => void
  onDragStart: (taskId: string) => void
  onDragEnd: () => void
}

/** List row: [checkbox] priority · id · status · title … labels · assignee · created. */
export function TaskRow({ task, statuses, labels, users, assignees, selected, dragging, onOpen, onToggleSelect, onDragStart, onDragEnd }: TaskRowProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const status = statuses.find((s) => s.id === task.statusId)
  const options = projectStatuses(statuses, task.projectId)
  return (
    <div
      className="list-row tasks-row"
      data-selected={selected || undefined}
      data-dragging={dragging || undefined}
      role="button"
      tabIndex={0}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/task-id', task.id)
        onDragStart(task.id)
      }}
      onDragEnd={onDragEnd}
      onClick={() => onOpen(task.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target === e.currentTarget) onOpen(task.id)
      }}
    >
      <label className="checkbox-box tasks-row-check" onClick={(e) => e.stopPropagation()}>
        <input type="checkbox" checked={selected} aria-label={`Select ${task.identifier}`} onChange={() => onToggleSelect(task.id)} />
        <span className="checkbox-box-visual" />
        <svg className="checkbox-box-tick" viewBox="0 0 12 12" fill="none" aria-hidden="true">
          <path d="M2.5 6.5 5 9l4.5-6" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </label>
      <PriorityPicker task={task} />
      <span className="tasks-row-id">{task.identifier}</span>
      <div onClick={(e) => e.stopPropagation()}>
        <Dropdown
          trigger={() => (
            <button className="icon-button tasks-row-status" aria-label={`Status: ${status?.name ?? 'None'}`}>
              <TaskStatusIcon status={status} />
            </button>
          )}
        >
          {(close) => (
            <>
              {options.map((option) => (
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
      </div>
      <span className="tasks-row-title truncate"><LinkifiedText text={task.title || 'Untitled'} /></span>
      {task.labels.length > 0 ? (
        <span className="tasks-row-labels">
          {task.labels.map((labelId) => {
            const label = labels.find((item) => item.id === labelId)
            return label ? <span key={label.id} className="pill"><span className="pill-dot" style={{ background: label.color }} />{label.name}</span> : null
          })}
        </span>
      ) : null}
      <div onClick={(e) => e.stopPropagation()}>
        <Dropdown
          align="right"
          trigger={() => (
            <button
              type="button"
              className="tasks-row-assignees"
              aria-label={assignees.length > 0 ? `Assignees: ${assignees.map((user) => user.name).join(', ')}` : 'Assign task'}
            >
              <AvatarStack users={assignees} size={18} />
            </button>
          )}
        >
          {() => (
            <>
              <div className="popover-heading">Assignees</div>
              {users.map((user) => {
                const active = task.assigneeIds.includes(user.id)
                return (
                  <button
                    key={user.id}
                    className="popover-option"
                    data-selected={active || undefined}
                    aria-pressed={active}
                    onClick={() => updateTask.mutate({
                      taskId: task.id,
                      body: {
                        expected_version: task.version,
                        assignee_ids: active
                          ? task.assigneeIds.filter((id) => id !== user.id)
                          : [...task.assigneeIds, user.id],
                      },
                    })}
                  >
                    <Avatar user={user} size={16} />
                    {user.name}
                    {active ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                  </button>
                )
              })}
            </>
          )}
        </Dropdown>
      </div>
      <span className="tasks-row-date">{shortDate(task.createdAt)}</span>
      {updateTask.isPending ? <span role="status" className="text-faint text-xs">Saving status…</span> : null}
      {updateTask.isError ? <span role="alert" className="text-danger text-xs">Status update failed. <button className="button button-ghost" onClick={(event) => { event.stopPropagation(); if (updateTask.variables) updateTask.mutate(updateTask.variables) }}>Retry</button></span> : null}
    </div>
  )
}
