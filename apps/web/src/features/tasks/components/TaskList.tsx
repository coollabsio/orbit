import { useState } from 'react'
import { Add, ChevronRight, TaskSquare, Xmark } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { EmptyState } from '../../../components/ui/EmptyState'
import { PriorityIcon } from '../../../components/workspace/PriorityIcon'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '../../../components/workspace/taskMeta'
import type { Task, TaskStatusDef, User } from '../api/models'
import { useBulkTasks, useUpdateTask } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
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
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
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

  const selectedTasks = tasks.filter((t) => selected.includes(t.id))

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
                if (statusId && statusId !== task.statusId) {
                  updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: statusId } })
                }
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
      {selectedTasks.length > 0 ? (
        <BulkBar
          tasks={selectedTasks}
          users={users}
          statuses={statuses}
          groups={groups}
          allTasks={tasks}
          onClear={() => setSelected([])}
        />
      ) : null}
    </>
  )
}

/** Floating toolbar for the selected rows: bulk status, priority, assignees and labels. */
function BulkBar({
  tasks,
  users,
  statuses,
  groups,
  allTasks,
  onClear,
}: {
  tasks: Task[]
  users: User[]
  statuses: TaskStatusDef[]
  groups: StatusGroup[]
  allTasks: Task[]
  onClear: () => void
}) {
  const { workspace } = useWorkspace()
  const bulkTasks = useBulkTasks(workspace.id)
  const allLabels = Array.from(new Set(allTasks.flatMap((t) => t.labels))).sort((a, b) => a.localeCompare(b))

  const bulkStatus = (key: string) => {
    bulkTasks.mutate(tasks.flatMap((task) => {
      const statusId = resolveStatusId(statuses, task.projectId, key)
      return statusId && statusId !== task.statusId
        ? [{ id: task.id, expected_version: task.version, status_id: statusId }]
        : []
    }))
  }
  const bulkPriority = (priority: Task['priority']) => {
    bulkTasks.mutate(tasks.filter((task) => task.priority !== priority).map((task) => ({ id: task.id, expected_version: task.version, priority })))
  }
  // everyone has it → remove from all; otherwise add to the tasks that miss it
  const bulkAssign = (userId: string) => {
    const everyone = tasks.every((t) => t.assigneeIds.includes(userId))
    bulkTasks.mutate(tasks.map((task) => ({
      id: task.id,
      expected_version: task.version,
      assignee_ids: everyone ? task.assigneeIds.filter((id) => id !== userId) : Array.from(new Set([...task.assigneeIds, userId])),
    })))
  }
  const bulkLabel = (label: string) => {
    const everyone = tasks.every((t) => t.labels.includes(label))
    bulkTasks.mutate(tasks.map((task) => ({
      id: task.id,
      expected_version: task.version,
      label_ids: everyone ? task.labels.filter((item) => item !== label) : Array.from(new Set([...task.labels, label])),
    })))
  }

  return (
    <div className="tasks-bulk-bar" role="toolbar" aria-label="Selected tasks">
      <span className="tasks-bulk-count">
        {tasks.length} selected
      </span>
      <Dropdown direction="up" trigger={() => <button className="button button-ghost">Status</button>}>
        {(close) => (
          <>
            {groups.map((group) => (
              <button
                key={group.key}
                className="popover-option"
                onClick={() => {
                  bulkStatus(group.key)
                  close()
                }}
              >
                <TaskStatusIcon status={group.status} />
                {group.name}
              </button>
            ))}
          </>
        )}
      </Dropdown>
      <Dropdown direction="up" trigger={() => <button className="button button-ghost">Priority</button>}>
        {(close) => (
          <>
            {PRIORITY_ORDER.map((priority) => (
              <button
                key={priority}
                className="popover-option"
                onClick={() => {
                  bulkPriority(priority)
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
      <Dropdown direction="up" align="right" trigger={() => <button className="button button-ghost">Assignee</button>}>
        {() => (
          <>
            {users.map((u) => {
              const everyone = tasks.every((t) => t.assigneeIds.includes(u.id))
              return (
                <button key={u.id} className="popover-option" data-selected={everyone || undefined} onClick={() => bulkAssign(u.id)}>
                  <Avatar user={u} size={16} />
                  {u.name}
                  {everyone ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                </button>
              )
            })}
          </>
        )}
      </Dropdown>
      <Dropdown direction="up" align="right" trigger={() => <button className="button button-ghost">Labels</button>}>
        {() => (
          <>
            {allLabels.map((label) => {
              const everyone = tasks.every((t) => t.labels.includes(label))
              return (
                <button key={label} className="popover-option" data-selected={everyone || undefined} onClick={() => bulkLabel(label)}>
                  <span className="pill">{label}</span>
                  {everyone ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                </button>
              )
            })}
          </>
        )}
      </Dropdown>
      <div className="spacer" />
      <button type="button" className="icon-button" aria-label="Clear selection" title="Clear selection" onClick={onClear}>
        <Xmark size={16} />
      </button>
    </div>
  )
}
