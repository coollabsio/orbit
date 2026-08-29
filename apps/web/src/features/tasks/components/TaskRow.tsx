import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL, STATUS_ORDER } from '../../../components/workspace/taskMeta'
import { relativeTime } from '../../../lib/format'
import { setTaskStatus } from '../../../mock/actions'
import type { Task, User } from '../../../mock/types'

interface TaskRowProps {
  task: Task
  assignee: User | undefined
  dragging: boolean
  onOpen: (taskId: string) => void
  onDragStart: (taskId: string) => void
  onDragEnd: () => void
}

export function TaskRow({ task, assignee, dragging, onOpen, onDragStart, onDragEnd }: TaskRowProps) {
  return (
    <div
      className="list-row tasks-row"
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
      <div onClick={(e) => e.stopPropagation()}>
        <Dropdown
          trigger={() => (
            <button className="icon-button" aria-label={`Status: ${STATUS_LABEL[task.status]}`}>
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
      <span className="tasks-row-id">{task.identifier}</span>
      <span className="tasks-row-title truncate">{task.title}</span>
      {task.labels.length > 0 ? (
        <span className="tasks-row-labels">
          {task.labels.map((label) => (
            <span key={label} className="pill">
              {label}
            </span>
          ))}
        </span>
      ) : null}
      <PriorityIcon priority={task.priority} />
      <Avatar user={assignee} size={18} name="—" />
      <span className="text-faint text-xs" style={{ flexShrink: 0 }}>
        {relativeTime(task.updatedAt)}
      </span>
    </div>
  )
}
