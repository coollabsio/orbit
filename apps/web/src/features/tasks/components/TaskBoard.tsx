import { Avatar } from '../../../components/ui/Avatar'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL, STATUS_ORDER } from '../../../components/workspace/taskMeta'
import { setTaskStatus } from '../../../mock/actions'
import type { Task, TaskStatus, User } from '../../../mock/types'

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
                const assignee = users.find((user) => user.id === task.assigneeId)
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
                      if (event.key === 'Enter') onOpen(task.id)
                    }}
                  >
                    <div className="tasks-board-card-topline">
                      <span>{task.identifier}</span>
                      <PriorityIcon priority={task.priority} />
                    </div>
                    <h3>{task.title}</h3>
                    {task.labels.length > 0 ? (
                      <div className="tasks-board-labels">
                        {task.labels.map((label) => (
                          <span key={label} className="pill">{label}</span>
                        ))}
                      </div>
                    ) : null}
                    <div className="tasks-board-card-footer">
                      <span className="tasks-board-comment-count">
                        {task.comments.length > 0 ? `${task.comments.length} comment${task.comments.length === 1 ? '' : 's'}` : ''}
                      </span>
                      <Avatar user={assignee} size={20} name="—" />
                    </div>
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
