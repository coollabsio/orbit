import { useState } from 'react'
import { Add, ChevronRight, TaskSquare } from 'reicon-react'
import { EmptyState } from '../../../components/ui/EmptyState'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { STATUS_LABEL } from '../../../components/workspace/taskMeta'
import { setTaskStatus } from '../../../mock/actions'
import type { Task, TaskStatus, User } from '../../../mock/types'
import { groupTasksByStatus } from '../tasksLib'
import { TaskRow } from './TaskRow'

interface TaskListProps {
  tasks: Task[]
  users: User[]
  onOpen: (taskId: string) => void
  onAdd: (status: TaskStatus) => void
}

/** Status groups: collapsible headers that also accept dropped rows (moves the task to that status). */
export function TaskList({ tasks, users, onOpen, onAdd }: TaskListProps) {
  const groups = groupTasksByStatus(tasks)
  const [collapsed, setCollapsed] = useState<TaskStatus[]>([])
  const [selected, setSelected] = useState<string[]>([])
  const [draggingId, setDraggingId] = useState<string | null>(null)
  const [dropStatus, setDropStatus] = useState<TaskStatus | null>(null)

  const toggle = (status: TaskStatus) =>
    setCollapsed((prev) => (prev.includes(status) ? prev.filter((s) => s !== status) : [...prev, status]))
  const toggleSelect = (taskId: string) =>
    setSelected((prev) => (prev.includes(taskId) ? prev.filter((id) => id !== taskId) : [...prev, taskId]))

  const endDrag = () => {
    setDraggingId(null)
    setDropStatus(null)
  }

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
      {groups.map((group) => {
        const isCollapsed = collapsed.includes(group.status)
        return (
          <section
            key={group.status}
            className="tasks-section"
            data-drop-over={dropStatus === group.status || undefined}
            onDragOver={(e) => {
              if (!draggingId) return
              e.preventDefault()
              e.dataTransfer.dropEffect = 'move'
              if (dropStatus !== group.status) setDropStatus(group.status)
            }}
            onDragLeave={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropStatus(null)
            }}
            onDrop={(e) => {
              e.preventDefault()
              const taskId = e.dataTransfer.getData('text/task-id') || draggingId
              const task = taskId ? tasks.find((t) => t.id === taskId) : undefined
              if (task && task.status !== group.status) setTaskStatus(task.id, group.status)
              endDrag()
            }}
          >
            <div className="tasks-section-header" data-collapsed={isCollapsed || undefined}>
              <button
                type="button"
                className="tasks-section-toggle"
                aria-expanded={!isCollapsed}
                aria-label={isCollapsed ? `Expand ${STATUS_LABEL[group.status]}` : `Collapse ${STATUS_LABEL[group.status]}`}
                onClick={() => toggle(group.status)}
              >
                <ChevronRight size={12} />
              </button>
              <TaskStatusIcon status={group.status} />
              <span>{STATUS_LABEL[group.status]}</span>
              <span className="tasks-section-count">{group.tasks.length}</span>
              <div className="spacer" />
              <button
                type="button"
                className="icon-button tasks-section-add"
                aria-label={`New task in ${STATUS_LABEL[group.status]}`}
                title="New task"
                onClick={() => onAdd(group.status)}
              >
                <Add size={14} />
              </button>
            </div>
            {!isCollapsed
              ? group.tasks.map((task) => (
                  <TaskRow
                    key={task.id}
                    task={task}
                    assignee={users.find((u) => u.id === task.assigneeId)}
                    selected={selected.includes(task.id)}
                    dragging={task.id === draggingId}
                    onOpen={onOpen}
                    onToggleSelect={toggleSelect}
                    onDragStart={setDraggingId}
                    onDragEnd={endDrag}
                  />
                ))
              : null}
          </section>
        )
      })}
    </>
  )
}
