import { useState } from 'react'
import { AvatarStack } from '../../../components/ui/Avatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import type { Task, TaskStatusDef, User } from '../api/models'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES, useBulkTasks } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { boardDropUpdates, resolveStatusId, sortTasks, type SortKey, type StatusGroup } from '../tasksLib'
import { PriorityPicker } from './PriorityPicker'
import { LabelPill } from './TaskLabels'

interface TaskBoardProps {
  tasks: Task[]
  users: User[]
  labels: LabelRecord[]
  statuses: TaskStatusDef[]
  groups: StatusGroup[]
  sort: SortKey
  activeTaskId: string | null
  onOpen: (taskId: string) => void
}

/** Kanban: one column per status group; dragging a card shows a placeholder where it will land. */
export function TaskBoard({ tasks, users, labels, statuses, groups, sort, activeTaskId, onOpen }: TaskBoardProps) {
  const { workspace } = useWorkspace()
  const bulkTasks = useBulkTasks(workspace.id)
  const limitError = bulkTasks.error instanceof BulkTaskLimitError ? bulkTasks.error : null
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
    const updates = boardDropUpdates(task, columnTasks, statusId, index)
    if (updates.length > 0) bulkTasks.mutate(updates)
  }

  return (
    <div className="tasks-board" style={{ gridTemplateColumns: `repeat(${groups.length}, 320px)` }}>
      {groups.map((group) => {
        const columnTasks = sortTasks(
          tasks.filter((task) => group.statusIds.includes(task.statusId)),
          sort,
        )
        const placeholderIndex = drop?.key === group.key ? drop.index : null
        // the dragged card stays mounted (faded): unmounting the drag source cancels the browser drag
        const others = dragging ? columnTasks.filter((t) => t.id !== dragging.id) : columnTasks
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
              {columnTasks.map((task) => {
                const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
                const isDragged = dragging?.id === task.id
                // placeholder slot index counts only the cards that can receive the drop
                const slot = isDragged ? -1 : others.indexOf(task)
                return (
                  <div key={task.id} style={{ display: 'contents' }}>
                    {!isDragged && placeholderIndex === slot ? (
                      <div className="tasks-board-placeholder" style={{ height: dragging?.height }} />
                    ) : null}
                    <article
                      className="tasks-board-card"
                      data-active={task.id === activeTaskId || undefined}
                      data-dragging={isDragged || undefined}
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
                          {assignees.length > 0 ? <AvatarStack users={assignees} size={18} /> : null}
                          <PriorityPicker task={task} align="right" />
                        </span>
                      </div>
                      <h3>{task.title || 'Untitled'}</h3>
                      {task.labels.length > 0 ? (
                        <div className="tasks-board-labels">
                          {task.labels.map((labelId) => {
                            const label = labels.find((item) => item.id === labelId)
                            return label ? <LabelPill key={label.id} label={label} /> : null
                          })}
                        </div>
                      ) : null}
                    </article>
                  </div>
                )
              })}
              {placeholderIndex !== null && placeholderIndex >= others.length ? (
                <div className="tasks-board-placeholder" style={{ height: dragging?.height }} />
              ) : null}
              {others.length === 0 && placeholderIndex === null ? <div className="tasks-board-empty">No tasks</div> : null}
            </div>
          </section>
        )
      })}
      {bulkTasks.isPending ? <p role="status" className="text-faint text-xs">Saving board order…</p> : null}
      {bulkTasks.isError ? <p role="alert" className="text-danger text-xs">{limitError ? `This move would update ${limitError.count} tasks. Move it in smaller steps so each drop affects at most ${MAX_BULK_TASK_UPDATES} tasks.` : <>Board reorder failed. <button className="button button-ghost" onClick={bulkTasks.retry}>Retry</button></>}</p> : null}
    </div>
  )
}
