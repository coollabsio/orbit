import { AvatarStack } from '../../../components/ui/Avatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL, STATUS_ORDER } from '../../../components/workspace/taskMeta'
import { setTaskStatus } from '../../../mock/actions'
import type { Task, TaskStatus, User } from '../../../mock/types'
import { PriorityPicker } from './PriorityPicker'

export function TaskBoard({
  tasks,
  users,
  activeTaskId,
  onOpen,
}: {
  tasks: Task[]
  users: User[]
  activeTaskId: string | null
  onOpen: (taskId: string) => void
}) {
  const moveTask = (event: React.DragEvent, status: TaskStatus) => {
    event.preventDefault()
    const taskId = event.dataTransfer.getData('text/task-id')
    const task = tasks.find((item) => item.id === taskId)
    if (task && task.status !== status) setTaskStatus(task.id, status)
  }

  return (
    <div className="tasks-board">
      {STATUS_ORDER.map((status) => {
        const columnTasks = tasks.filter((task) => task.status === status)
        return (
          <section
            key={status}
            className="tasks-board-column"
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => moveTask(event, status)}
          >
            <header className="tasks-board-column-header">
              <TaskStatusIcon status={status} />
              <span>{STATUS_LABEL[status]}</span>
              <span className="tasks-board-count">{columnTasks.length}</span>
            </header>
            <div className="tasks-board-cards">
              {columnTasks.map((task) => {
                const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
                return (
                  <article
                    key={task.id}
                    className="tasks-board-card"
                    data-active={task.id === activeTaskId || undefined}
                    draggable
                    tabIndex={0}
                    onDragStart={(event) => {
                      event.dataTransfer.effectAllowed = 'move'
                      event.dataTransfer.setData('text/task-id', task.id)
                    }}
                    onClick={() => onOpen(task.id)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' && event.target === event.currentTarget) onOpen(task.id)
                    }}
                  >
                    {/* id … assignees · priority (priority changes in place) */}
                    <div className="tasks-board-card-topline">
                      <span>{task.identifier}</span>
                      <span className="tasks-board-card-meta">
                        <AvatarStack users={assignees} size={18} />
                        <PriorityPicker task={task} align="right" />
                      </span>
                    </div>
                    <h3>{task.title || 'Untitled'}</h3>
                    {task.labels.length > 0 ? (
                      <div className="tasks-board-labels">
                        {task.labels.map((label) => (
                          <span key={label} className="pill">{label}</span>
                        ))}
                      </div>
                    ) : null}
                  </article>
                )
              })}
              {columnTasks.length === 0 ? <div className="tasks-board-empty">No tasks</div> : null}
            </div>
          </section>
        )
      })}
    </div>
  )
}
