import { AvatarStack } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL, STATUS_ORDER } from '../../../components/workspace/taskMeta'
import { shortDate } from '../../../lib/format'
import { setTaskStatus } from '../../../mock/actions'
import type { Task, User } from '../../../mock/types'
import { PriorityPicker } from './PriorityPicker'

interface TaskRowProps {
  task: Task
  assignees: User[]
  selected: boolean
  dragging: boolean
  onOpen: (taskId: string) => void
  onToggleSelect: (taskId: string) => void
  onDragStart: (taskId: string) => void
  onDragEnd: () => void
}

/** List row: [checkbox] priority · id · status · title … labels · assignee · created. */
export function TaskRow({ task, assignees, selected, dragging, onOpen, onToggleSelect, onDragStart, onDragEnd }: TaskRowProps) {
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
            <button className="icon-button tasks-row-status" aria-label={`Status: ${STATUS_LABEL[task.status]}`}>
              <TaskStatusIcon status={task.status} />
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
      <span className="tasks-row-title truncate">{task.title || 'Untitled'}</span>
      {task.labels.length > 0 ? (
        <span className="tasks-row-labels">
          {task.labels.map((label) => (
            <span key={label} className="pill">
              {label}
            </span>
          ))}
        </span>
      ) : null}
      <AvatarStack users={assignees} size={18} />
      <span className="tasks-row-date">{shortDate(task.createdAt)}</span>
    </div>
  )
}
