import { useState } from 'react'
import { ChevronRight } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import type { LabelRecord } from '@/api/generated/types.gen'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { pickerTitle } from '@/features/tasks/relationsLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { CHEVRON, GroupIcon } from '@/features/views/components/GroupIcon'
import type { GroupContext, TaskGroup } from '@/features/views/grouping'
import { boardGrid, canDrag, cellTasks, valuesOf, zoneIdOf, type GroupValues } from '@/features/views/layoutGroups'
import { useCollapsedGroups } from '@/features/views/useCollapsedGroups'
import { useGroupDrop } from '@/features/views/useGroupDrop'
import type { DisplayOptions, TaskProperty } from '@/features/views/viewState'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { BlockedIndicator } from './BlockedIndicator'
import { PriorityPicker } from './PriorityPicker'
import { LabelPill } from './TaskLabels'
import { TaskPickerDialog } from './TaskPickerDialog'
import { DateStamp, DueDateChip, ProjectChip } from './TaskPropertyChips'
import { TaskStatusIcon } from './TaskStatusIcon'

export interface TaskBoardProps {
  tasks: Task[]
  users: User[]
  labels: LabelRecord[]
  statuses: TaskStatusDef[]
  projects: Project[]
  display: DisplayOptions
  groupContext: GroupContext
  /** Page scope for collapsed swim lanes (see TaskList). Key the board by it. */
  collapseScope: string
  activeTaskId: string | null
  onOpen: (taskId: string) => void
}

const COLUMN_WIDTH = 320
const ZONE_RING = 'data-[drop-over]:ring-1 data-[drop-over]:ring-primary/40 data-[drop-over]:ring-inset'
const PLACEHOLDER = 'min-h-11 rounded-md border border-dashed border-primary/40 bg-primary/10'
/** Lift only where hover is real and motion is welcome; transition named properties, never `all`. */
const CARD =
  'flex cursor-pointer flex-col gap-[7px] rounded-md border border-border bg-card p-2.5 transition-[translate,background-color,border-color,box-shadow,opacity] duration-150 ease-out hover-fine:hover:border-foreground/20 hover-fine:hover:bg-accent hover-fine:hover:shadow-md motion-safe:hover-fine:hover:-translate-y-px data-[dragging]:border-dashed data-[dragging]:opacity-35 data-[active]:border-primary/40 focus-visible:ring-1 focus-visible:ring-primary focus-visible:outline-none'
/**
 * Column headers are 12px padding + 38px tall; lane headers stick right under them. A collapsed lane is a
 * drop zone itself, tinted like a list group header while a card is over it.
 */
const LANE_HEADER =
  'group/lane sticky top-[50px] z-10 col-span-full flex h-9 items-center border-b border-border bg-background transition-colors duration-150 data-[drop-over]:bg-primary/10'
/**
 * Sticks to the left edge while the board scrolls sideways, so the lane name stays in view. Hover only under
 * a fine pointer; a press scales to 0.97 instead of the default 1px nudge.
 */
const LANE_LABEL =
  'sticky left-3 h-7 gap-2 px-1.5 text-xs font-semibold text-muted-foreground transition-[color,background-color,scale] duration-150 ease-out hover-fine:hover:text-foreground group-data-[drop-over]/lane:text-primary active:not-aria-[haspopup]:translate-y-0 active:scale-[0.97] motion-reduce:active:scale-100'

/** Kanban: columns from the grouping, optional swim lanes from the sub-grouping; drops rewrite both fields. */
export function TaskBoard({ tasks, users, labels, statuses, projects, display, groupContext, collapseScope, activeTaskId, onOpen }: TaskBoardProps) {
  const { workspace } = useWorkspace()
  const duplicates = useDuplicateActions(workspace.id)
  const [duplicateTask, setDuplicateTask] = useState<Task | null>(null)
  const [collapsed, toggle] = useCollapsedGroups(`orbit:task_board_lanes_collapsed:${workspace.id}:${collapseScope}`)
  const manual = display.order_by === 'manual'
  const { drag, drop, startDrag, endDrag, zoneProps, saving } = useGroupDrop({
    tasks,
    manual,
    groupContext,
    itemSelector: '[data-board-card]',
    onDuplicate: setDuplicateTask,
  })
  // the placeholder takes the dragged card's height
  const [dragHeight, setDragHeight] = useState(0)

  const dragEnabled = canDrag(display)
  const { columns, lanes } = boardGrid(tasks, display, groupContext)
  const has = (property: TaskProperty) => display.properties.includes(property)
  const statusById = new Map(statuses.map((status) => [status.id, status]))
  const projectById = new Map(projects.map((project) => [project.id, project]))
  const labelById = new Map(labels.map((label) => [label.id, label]))

  const renderCard = (task: Task, values: GroupValues) => {
    const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
    const status = statusById.get(task.statusId)
    const cardLabels = has('labels') ? task.labels.flatMap((id) => labelById.get(id) ?? []) : []
    const showTop = has('status') || has('id') || Boolean(task.blocked) || (has('assignee') && assignees.length > 0) || has('priority')
    const showMeta = has('project') || (has('due_date') && Boolean(task.dueAt)) || has('created') || has('updated')
    return (
      <article
        data-board-card
        className={CARD}
        data-active={task.id === activeTaskId || undefined}
        data-dragging={drag?.taskId === task.id || undefined}
        draggable={dragEnabled}
        tabIndex={0}
        onDragStart={dragEnabled ? (event) => {
          event.dataTransfer.effectAllowed = 'move'
          event.dataTransfer.setData('text/task-id', task.id)
          setDragHeight(event.currentTarget.offsetHeight)
          startDrag(task.id, values)
        } : undefined}
        onDragEnd={endDrag}
        onClick={() => onOpen(task.id)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && event.target === event.currentTarget) onOpen(task.id)
        }}
      >
        {/* status · id · blocked … assignees · priority (priority changes in place) */}
        {showTop ? (
          <div className="flex min-h-5 items-center justify-between gap-2 text-[11px] text-muted-foreground/70">
            <span className="flex min-w-0 items-center gap-1">
              {has('status') ? <TaskStatusIcon status={status} size={12} /> : null}
              {has('id') ? <span className="whitespace-nowrap tabular-nums">{task.identifier}</span> : null}
              {task.blocked ? <BlockedIndicator /> : null}
            </span>
            <span className="flex items-center gap-1">
              {has('assignee') && assignees.length > 0 ? <UserAvatarStack users={assignees} size={18} /> : null}
              {has('priority') ? <PriorityPicker task={task} align="right" /> : null}
            </span>
          </div>
        ) : null}
        <h3 className="text-[13px] leading-[18px] font-medium text-foreground">{task.title || 'Untitled'}</h3>
        {cardLabels.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {cardLabels.map((label) => <LabelPill key={label.id} label={label} />)}
          </div>
        ) : null}
        {showMeta ? (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground/70">
            {has('project') ? <ProjectChip project={projectById.get(task.projectId)} /> : null}
            {has('due_date') ? <DueDateChip task={task} status={status} /> : null}
            {has('created') ? <DateStamp property="created" iso={task.createdAt} /> : null}
            {has('updated') ? <DateStamp property="updated" iso={task.updatedAt} /> : null}
          </div>
        ) : null}
      </article>
    )
  }

  const renderCards = (zone: string, values: GroupValues, cell: Task[], emptyLabel: boolean) => {
    const placeholderIndex = drop?.zone === zone ? drop.index : null
    // the dragged card stays mounted (faded): unmounting the drag source cancels the browser drag
    const others = drag ? cell.filter((task) => task.id !== drag.taskId) : cell
    const placeholder = <div className={PLACEHOLDER} style={{ height: dragHeight }} />
    return (
      <>
        {cell.map((task) => {
          const isDragged = drag?.taskId === task.id
          // placeholder slot index counts only the cards that can receive the drop
          const slot = isDragged ? -1 : others.indexOf(task)
          return (
            <div key={task.id} className="contents">
              {!isDragged && placeholderIndex === slot ? placeholder : null}
              {renderCard(task, values)}
            </div>
          )
        })}
        {placeholderIndex !== null && placeholderIndex >= others.length ? placeholder : null}
        {emptyLabel && others.length === 0 && drop?.zone !== zone ? <div className="flex h-[72px] items-center justify-center text-xs text-muted-foreground/70">No tasks</div> : null}
      </>
    )
  }

  const columnHeader = (column: TaskGroup) => (
    <header className="flex h-[38px] items-center gap-[7px] px-2.5 text-xs font-semibold text-muted-foreground transition-colors duration-150 group-data-[drop-over]/col:text-primary">
      <GroupIcon group={column} context={groupContext} />
      <span className="truncate">{column.label}</span>
      <span className="ml-auto font-normal text-muted-foreground/70 tabular-nums">{column.tasks.length}</span>
    </header>
  )

  const gridStyle = { gridTemplateColumns: `repeat(${columns.length}, ${COLUMN_WIDTH}px)` }
  const picker = duplicateTask ? (
    <TaskPickerDialog
      open
      onOpenChange={(open) => { if (!open) setDuplicateTask(null) }}
      title={pickerTitle('duplicate', duplicateTask.identifier)}
      statuses={statuses}
      excludeIds={[duplicateTask.id]}
      excludeDuplicates
      onSelect={(target) => void duplicates.markOne(duplicateTask, target)}
    />
  ) : null

  if (!lanes) {
    return (
      <div className="relative grid min-h-full min-w-max gap-3 p-3" aria-busy={saving} style={gridStyle}>
        {columns.map((column) => {
          const values = valuesOf(column)
          const zone = zoneIdOf(values)
          return (
            <section key={column.key} className={cn('group/col min-w-0 rounded-md bg-muted/40 transition-shadow', ZONE_RING)} {...zoneProps({ id: zone, values, tasks: column.tasks, itemsShown: true })}>
              {columnHeader(column)}
              <div className="flex min-h-[120px] flex-col gap-[7px] px-[7px] pb-[7px]">{renderCards(zone, values, column.tasks, true)}</div>
            </section>
          )
        })}
        {picker}
      </div>
    )
  }

  return (
    <div className="relative grid min-h-full min-w-max content-start gap-x-3 px-3 pb-3" aria-busy={saving} style={gridStyle}>
      <div className="sticky top-0 z-20 col-span-full grid grid-cols-subgrid bg-background pt-3">
        {columns.map((column) => <div key={column.key} className="min-w-0 rounded-t-md bg-muted/40">{columnHeader(column)}</div>)}
      </div>
      {lanes.map((lane) => {
        const laneValues = valuesOf(lane)
        const laneId = zoneIdOf(laneValues)
        const open = !collapsed.includes(laneId)
        // collapsed: the cells are hidden, so the lane header takes drops; only the lane field changes and the
        // card goes to the lane's end (a hidden column value is never written)
        const laneDrop = open ? {} : zoneProps({ id: laneId, values: laneValues, tasks: lane.tasks, itemsShown: false })
        return (
          <section key={lane.key} aria-label={lane.label} className="col-span-full grid grid-cols-subgrid">
            <div className={LANE_HEADER} {...laneDrop}>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className={LANE_LABEL}
                aria-expanded={open}
                aria-label={`${open ? 'Collapse' : 'Expand'} ${lane.label}`}
                onClick={() => toggle(laneId)}
              >
                <ChevronRight aria-hidden className={cn(CHEVRON, open && 'rotate-90')} />
                <GroupIcon group={lane} context={groupContext} />
                <span>{lane.label}</span>
                <span className="font-normal text-muted-foreground/70 tabular-nums">{lane.tasks.length}</span>
              </Button>
            </div>
            {open ? columns.map((column) => {
              const values = [...valuesOf(column), ...laneValues]
              const zone = zoneIdOf(values)
              const cell = cellTasks(column, lane)
              return (
                <div
                  key={column.key}
                  data-board-cell={zone}
                  className={cn('my-2 flex min-h-[72px] min-w-0 flex-col gap-[7px] rounded-md bg-muted/40 p-[7px] transition-shadow', ZONE_RING)}
                  {...zoneProps({ id: zone, values, tasks: cell, itemsShown: true })}
                >
                  {renderCards(zone, values, cell, false)}
                </div>
              )
            }) : null}
          </section>
        )
      })}
      {picker}
    </div>
  )
}
