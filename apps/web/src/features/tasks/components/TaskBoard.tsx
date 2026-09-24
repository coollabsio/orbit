import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { TaskStatusIcon } from './TaskStatusIcon'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { LabelRecord } from '@/api/generated/types.gen'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES, useBulkTasks } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { boardDropUpdates, resolveStatusId, sortTasks, type SortKey, type StatusGroup } from '@/features/tasks/tasksLib'
import { PriorityPicker } from './PriorityPicker'
import { LabelPill } from './TaskLabels'
import { pickerTitle } from '@/features/tasks/relationsLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { BlockedIndicator } from './BlockedIndicator'
import { TaskPickerDialog } from './TaskPickerDialog'

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
  const duplicates = useDuplicateActions(workspace.id)
  const [duplicateTask, setDuplicateTask] = useState<Task | null>(null)

  const endDrag = () => {
    setDragging(null)
    setDrop(null)
  }

  /** Index in the column's card list where the pointer currently is (cards' vertical midpoints). */
  const indexAt = (column: HTMLElement, clientY: number) => {
    const cards = Array.from(column.querySelectorAll<HTMLElement>('[data-board-card]:not([data-dragging="true"])'))
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
    // entering Duplicate needs a canonical task: ask first; nothing is written until one is picked
    if (statusId !== task.statusId && statuses.find((status) => status.id === statusId)?.category === 'duplicate') {
      setDuplicateTask(task)
      return
    }
    const updates = boardDropUpdates(task, columnTasks, statusId, index)
    if (updates.length > 0) bulkTasks.mutate(updates)
  }

  return (
    <div className="relative grid min-h-full min-w-max gap-3 p-3" aria-busy={bulkTasks.isPending} style={{ gridTemplateColumns: `repeat(${groups.length}, 320px)` }}>
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
            className="group/col min-w-0 rounded-md bg-muted/40 transition-shadow data-[drop-over]:ring-1 data-[drop-over]:ring-primary/40 data-[drop-over]:ring-inset"
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
            <header className="flex h-[38px] items-center gap-[7px] px-2.5 text-xs font-semibold text-muted-foreground group-data-[drop-over]/col:text-primary">
              <TaskStatusIcon status={group.status} />
              <span>{group.name}</span>
              <span className="ml-auto font-normal text-muted-foreground/70">{columnTasks.length}</span>
            </header>
            <div className="flex min-h-[120px] flex-col gap-[7px] px-[7px] pb-[7px]">
              {columnTasks.map((task) => {
                const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
                const isDragged = dragging?.id === task.id
                // placeholder slot index counts only the cards that can receive the drop
                const slot = isDragged ? -1 : others.indexOf(task)
                return (
                  <div key={task.id} className="contents">
                    {!isDragged && placeholderIndex === slot ? (
                      <div className="min-h-11 rounded-md border border-dashed border-primary/40 bg-primary/10" style={{ height: dragging?.height }} />
                    ) : null}
                    <article
                      data-board-card
                      className="cursor-pointer rounded-md border border-border bg-card p-2.5 transition-all hover:-translate-y-px hover:border-foreground/20 hover:bg-accent hover:shadow-md data-[dragging]:border-dashed data-[dragging]:opacity-35 data-[active]:border-primary/40 focus-visible:ring-1 focus-visible:ring-primary focus-visible:outline-none"
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
                      <div className="flex min-h-5 items-center justify-between text-[11px] text-muted-foreground/70">
                        <span className="flex items-center gap-1">
                          {task.identifier}
                          {task.blocked ? <BlockedIndicator /> : null}
                        </span>
                        <span className="flex items-center gap-1">
                          {assignees.length > 0 ? <UserAvatarStack users={assignees} size={18} /> : null}
                          <PriorityPicker task={task} align="right" />
                        </span>
                      </div>
                      <h3 className="mt-[5px] mb-[9px] text-[13px] leading-[18px] font-medium text-foreground">{task.title || 'Untitled'}</h3>
                      {task.labels.length > 0 ? (
                        <div className="mb-[9px] flex flex-wrap gap-1">
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
                <div className="min-h-11 rounded-md border border-dashed border-primary/40 bg-primary/10" style={{ height: dragging?.height }} />
              ) : null}
              {others.length === 0 && placeholderIndex === null ? <div className="flex h-[72px] items-center justify-center text-xs text-muted-foreground/70">No tasks</div> : null}
            </div>
          </section>
        )
      })}
      {bulkTasks.isError ? <p role="alert" className="absolute top-3 right-3 z-10 rounded-md border border-destructive/30 bg-background px-2 py-1 text-xs text-destructive shadow-sm">{limitError ? `This move would update ${limitError.count} tasks. Move it in smaller steps so each drop affects at most ${MAX_BULK_TASK_UPDATES} tasks.` : <>Board reorder failed. <Button variant="ghost" onClick={bulkTasks.retry}>Retry</Button></>}</p> : null}
      {duplicateTask ? (
        <TaskPickerDialog
          open
          onOpenChange={(open) => { if (!open) setDuplicateTask(null) }}
          title={pickerTitle('duplicate', duplicateTask.identifier)}
          statuses={statuses}
          excludeIds={[duplicateTask.id]}
          excludeDuplicates
          onSelect={(target) => void duplicates.markOne(duplicateTask, target)}
        />
      ) : null}
    </div>
  )
}
