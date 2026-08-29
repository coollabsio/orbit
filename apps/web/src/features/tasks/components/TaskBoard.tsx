import { useState } from 'react'
import { AvatarStack } from '../../../components/ui/Avatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { moveTask } from '../../../mock/actions'
import type { Task, TaskStatusDef, User } from '../../../mock/types'
import { resolveStatusId, sortTasks, type SortKey, type StatusGroup } from '../tasksLib'
import { PriorityPicker } from './PriorityPicker'

interface TaskBoardProps {
  tasks: Task[]
  users: User[]
  statuses: TaskStatusDef[]
  groups: StatusGroup[]
  sort: SortKey
  activeTaskId: string | null
  onOpen: (taskId: string) => void
}

/** Kanban: one column per status group; dragging a card shows a placeholder where it will land. */
export function TaskBoard({ tasks, users, statuses, groups, sort, activeTaskId, onOpen }: TaskBoardProps) {
  const [dragging, setDragging] = useState<{ id: string; height: number } | null>(null)
  const [drop, setDrop] = useState<{ key: string; index: number } | null>(null)

  const endDrag = () => {
    setDragging(null)
    setDrop(null)
  }

  /** Index in the column's card list where the pointer currently is (cards' vertical midpoints). */
  const indexAt = (column: HTMLElement, clientY: number) => {
    const cards = Array.from(column.querySelectorAll<HTMLElement>('.tasks-board-card:not([data-dragging="true"])'))
    const below = cards.findIndex((card) => {
      const rect = card.getBoundingClientRect()
      return clientY < rect.top + rect.height / 2
    })
    return below === -1 ? cards.length : below
  }

  const dropInto = (group: StatusGroup, columnTasks: Task[], index: number) => {
    const task = dragging ? tasks.find((t) => t.id === dragging.id) : undefined
    if (!task) return
    const statusId = resolveStatusId(statuses, task.projectId, group.key)
    if (!statusId) return
    // position between the neighbours at the drop index (manual order); other sorts still change the status
    const others = columnTasks.filter((t) => t.id !== task.id)
    const prev = others[index - 1]
    const next = others[index]
    const position = prev && next ? (prev.position + next.position) / 2 : prev ? prev.position + 1 : next ? next.position - 1 : 0
    if (statusId !== task.statusId || position !== task.position) moveTask(task.id, statusId, position)
  }

  return (
    <div className="tasks-board" style={{ gridTemplateColumns: `repeat(${groups.length}, minmax(220px, 1fr))` }}>
      {groups.map((group) => {
        const columnTasks = sortTasks(
          tasks.filter((task) => group.statusIds.includes(task.statusId)),
          sort,
        )
        const placeholderIndex = drop?.key === group.key ? drop.index : null
        const visible = dragging ? columnTasks.filter((t) => t.id !== dragging.id) : columnTasks
        return (
          <section
            key={group.key}
            className="tasks-board-column"
            data-drop-over={placeholderIndex !== null || undefined}
            onDragOver={(event) => {
              if (!dragging) return
              event.preventDefault()
              event.dataTransfer.dropEffect = 'move'
              const index = indexAt(event.currentTarget, event.clientY)
              if (drop?.key !== group.key || drop.index !== index) setDrop({ key: group.key, index })
            }}
            onDragLeave={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget as Node | null) && drop?.key === group.key) setDrop(null)
            }}
            onDrop={(event) => {
              event.preventDefault()
              dropInto(group, columnTasks, indexAt(event.currentTarget, event.clientY))
              endDrag()
            }}
          >
            <header className="tasks-board-column-header">
              <TaskStatusIcon status={group.status} />
              <span>{group.name}</span>
              <span className="tasks-board-count">{columnTasks.length}</span>
            </header>
            <div className="tasks-board-cards">
              {visible.map((task, index) => {
                const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
                return (
                  <div key={task.id} style={{ display: 'contents' }}>
                    {placeholderIndex === index ? <div className="tasks-board-placeholder" style={{ height: dragging?.height }} /> : null}
                    <article
                      className="tasks-board-card"
                      data-active={task.id === activeTaskId || undefined}
                      data-dragging={dragging?.id === task.id || undefined}
                      draggable
                      tabIndex={0}
                      onDragStart={(event) => {
                        event.dataTransfer.effectAllowed = 'move'
                        event.dataTransfer.setData('text/task-id', task.id)
                        setDragging({ id: task.id, height: event.currentTarget.offsetHeight })
                      }}
                      onDragEnd={endDrag}
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
                  </div>
                )
              })}
              {placeholderIndex !== null && placeholderIndex >= visible.length ? (
                <div className="tasks-board-placeholder" style={{ height: dragging?.height }} />
              ) : null}
              {visible.length === 0 && placeholderIndex === null ? <div className="tasks-board-empty">No tasks</div> : null}
            </div>
          </section>
        )
      })}
    </div>
  )
}
