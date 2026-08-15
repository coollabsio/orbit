import { TaskSquare } from 'reicon-react'
import { EmptyState } from '../../../components/ui/EmptyState'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL } from '../../../components/workspace/taskMeta'
import type { Task, User } from '../../../mock/types'
import { groupTasksByStatus } from '../tasksLib'
import { TaskRow } from './TaskRow'

interface TaskListProps {
  tasks: Task[]
  users: User[]
  activeTaskId: string | null
  onOpen: (taskId: string) => void
}

export function TaskList({ tasks, users, activeTaskId, onOpen }: TaskListProps) {
  const groups = groupTasksByStatus(tasks)

  if (groups.length === 0) {
    return (
      <EmptyState
        icon={TaskSquare}
        title="No tasks found"
        description="No tasks match the current filters. Try clearing a filter or create a new task."
      />
    )
  }

  return (
    <>
      {groups.map((group) => (
        <section key={group.status}>
          <div className="tasks-section-header">
            <TaskStatusIcon status={group.status} />
            {STATUS_LABEL[group.status]}
            <span className="text-faint" style={{ fontWeight: 400 }}>
              {group.tasks.length}
            </span>
          </div>
          {group.tasks.map((task) => (
            <TaskRow
              key={task.id}
              task={task}
              assignee={users.find((u) => u.id === task.assigneeId)}
              active={task.id === activeTaskId}
              onOpen={onOpen}
            />
          ))}
        </section>
      ))}
    </>
  )
}
