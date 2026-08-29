import { useState } from 'react'
import { Add, ChevronRight, TaskSquare } from 'reicon-react'
import { EmptyState } from '../../../components/ui/EmptyState'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { setTaskStatus } from '../../../mock/actions'
import type { Task, TaskStatusDef, User } from '../../../mock/types'
import { groupTasksByStatus, resolveStatusId, type SortKey, type StatusGroup } from '../tasksLib'
import { TaskRow } from './TaskRow'

interface TaskListProps {
  tasks: Task[]
  users: User[]
  statuses: TaskStatusDef[]
  groups: StatusGroup[]
  sort: SortKey
  onOpen: (taskId: string) => void
  onAdd: (statusKey: string) => void
}

/** Status groups: collapsible headers that also accept dropped rows (moves the task to that status). */
export function TaskList({ tasks, users, statuses, groups, sort, onOpen, onAdd }: TaskListProps) {
  const taskGroups = groupTasksByStatus(tasks, groups, sort)
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [selected, setSelected] = useState<string[]>([])
  const [draggingId, setDraggingId] = useState<string | null>(null)
  const [dropKey, setDropKey] = useState<string | null>(null)

  const toggle = (key: string) => setCollapsed((prev) => (prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key]))
  const toggleSelect = (taskId: string) =>
    setSelected((prev) => (prev.includes(taskId) ? prev.filter((id) => id !== taskId) : [...prev, taskId]))

  const endDrag = () => {
    setDraggingId(null)
    setDropKey(null)
  }

  if (taskGroups.length === 0) {
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
      {taskGroups.map((group) => {
        const isCollapsed = collapsed.includes(group.key)
        return (
          <section
            key={group.key}
            className="tasks-section"
            data-drop-over={dropKey === group.key || undefined}
            onDragOver={(e) => {
              if (!draggingId) return
              e.preventDefault()
              e.dataTransfer.dropEffect = 'move'
              if (dropKey !== group.key) setDropKey(group.key)
            }}
            onDragLeave={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropKey(null)
            }}
            onDrop={(e) => {
              e.preventDefault()
              const taskId = e.dataTransfer.getData('text/task-id') || draggingId
              const task = taskId ? tasks.find((t) => t.id === taskId) : undefined
              if (task) {
                // the dropped task moves to the status of its own project that matches this group
                const statusId = resolveStatusId(statuses, task.projectId, group.key)
                if (statusId && statusId !== task.statusId) setTaskStatus(task.id, statusId)
              }
              endDrag()
            }}
          >
            <div className="tasks-section-header" data-collapsed={isCollapsed || undefined}>
              <button
                type="button"
                className="tasks-section-toggle"
                aria-expanded={!isCollapsed}
                aria-label={isCollapsed ? `Expand ${group.name}` : `Collapse ${group.name}`}
                onClick={() => toggle(group.key)}
              >
                <ChevronRight size={12} />
              </button>
              <TaskStatusIcon status={group.status} />
              <span>{group.name}</span>
              <span className="tasks-section-count">{group.tasks.length}</span>
              <div className="spacer" />
              <button
                type="button"
                className="icon-button tasks-section-add"
                aria-label={`New task in ${group.name}`}
                title="New task"
                onClick={() => onAdd(group.key)}
              >
                <Add size={14} />
              </button>
            </div>
            {!isCollapsed
              ? group.tasks.map((task) => (
                  <TaskRow
                    key={task.id}
                    task={task}
                    statuses={statuses}
                    assignees={users.filter((u) => task.assigneeIds.includes(u.id))}
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
