import { focusAdjacentColumn, taskRowTarget, useVirtualTaskRows } from '@/shortcuts/taskTarget'
import { memo, useCallback, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { defaultRangeExtractor, measureElement as measureSize, useVirtualizer } from '@tanstack/react-virtual'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import type { LabelRecord } from '@orbit/api/generated/types.gen'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { pickerTitle } from '@/features/tasks/relationsLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { DisclosureChevron, GroupIcon } from '@/features/views/components/GroupIcon'
import { estimateTotal, type GroupContext, type TaskGroup } from '@/features/views/grouping'
import { estimateShortLabel } from '@/features/tasks/planningLib'
import { boardGrid, canDrag, cellTasks, valuesOf, zoneIdOf, type GroupValues } from '@/features/views/layoutGroups'
import { useCollapsedGroups } from '@/features/views/useCollapsedGroups'
import { useGroupDrop } from '@/features/views/useGroupDrop'
import { useScrollMemory } from '@/lib/useScrollMemory'
import { useNestDrop } from '@/features/views/useNestDrop'
import { useParentActions } from '@/features/tasks/useParentActions'
import type { DisplayOptions, TaskProperty } from '@/features/views/viewState'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { AssigneePicker } from './AssigneePicker'
import { BlockedIndicator } from './BlockedIndicator'
import { PriorityPicker } from './PriorityPicker'
import { SubIssueProgress, completedStatusColor } from './SubIssueProgress'
import { LabelPill } from './TaskLabels'
import { NestChip } from './NestChip'
import { TaskPickerDialog } from './TaskPickerDialog'
import { CycleChip, DateStamp, EstimateChip, MilestoneChip, MilestoneLate, ProjectChip } from './TaskPropertyChips'
import { DueDatePicker } from './DueDatePicker'
import { StatusPicker } from './StatusPicker'

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
/** The height of a card before any card is measured, and the space between cards. */
const CARD_HEIGHT = 110
const CARD_GAP = 7
/** A zone this far outside the view (px) mounts no cards. */
const ZONE_MARGIN = 400
/**
 * Measured card heights by shown properties and task, kept while the board is unmounted (a task is open): back on the
 * board, the cards get their real heights at once, so the remembered scroll position shows the same cards.
 */
const cardSizes = new Map<string, number>()
/** The sum and count of the measured heights per set of shown properties: the estimate for a card not measured yet. */
const sizeTotals = new Map<string, { sum: number; count: number }>()
function recordSize(sizeKey: string, taskId: string, size: number) {
  const key = `${sizeKey}:${taskId}`
  const previous = cardSizes.get(key)
  const totals = sizeTotals.get(sizeKey) ?? { sum: 0, count: 0 }
  sizeTotals.set(sizeKey, previous === undefined ? { sum: totals.sum + size, count: totals.count + 1 } : { sum: totals.sum - previous + size, count: totals.count })
  cardSizes.set(key, size)
}
function estimatedSize(sizeKey: string, taskId: string) {
  const totals = sizeTotals.get(sizeKey)
  return cardSizes.get(`${sizeKey}:${taskId}`) ?? (totals ? totals.sum / totals.count : CARD_HEIGHT)
}
/**
 * Kanban: columns from the grouping, optional swim lanes from the sub-grouping; drops rewrite both fields.
 * The board has its own scroll. Each column (or lane cell) is a TanStack virtual list on that scroller, so only the
 * cards near the view are in the DOM.
 */
export function TaskBoard({ tasks, users, labels, statuses, projects, display, groupContext, collapseScope, activeTaskId, onOpen }: TaskBoardProps) {
  const { workspace } = useWorkspace()
  const duplicates = useDuplicateActions(workspace.id)
  const [duplicateTask, setDuplicateTask] = useState<Task | null>(null)
  const [collapsed, toggle] = useCollapsedGroups(`orbit:task_board_lanes_collapsed:${workspace.id}:${collapseScope}`)
  const manual = display.order_by === 'manual'
  const { drag, drop, startDrag, endDrag, holdDrop, zoneProps, saving } = useGroupDrop({
    tasks,
    manual,
    groupContext,
    itemSelector: '[data-board-card]',
    onDuplicate: setDuplicateTask,
  })
  const parentActions = useParentActions(workspace.id)
  // the board renders nested as flat: a card's middle nests, its edges keep the column/lane drop
  const nest = useNestDrop({
    tasks,
    tree: null,
    manual,
    dragId: drag?.taskId ?? null,
    // hold, not clear: removing the in-flow placeholder would shift the cards under the pointer (flicker)
    onTakeOver: holdDrop,
    endDrag,
    onNest: (plan, dragId) => {
      const moving = tasks.find((task) => task.id === dragId)
      const parent = tasks.find((task) => task.id === plan.parentId)
      if (!moving || !parent) return
      void parentActions.setParent([moving], { id: parent.id, identifier: parent.identifier }, { placement: plan.placement })
    },
  })
  // the placeholder takes the dragged card's height
  const [dragHeight, setDragHeight] = useState(0)
  // the card whose header date picker is open: it stays in the header after the first pick gives the task a date,
  // so the picker is not unmounted while a range or a time is still being picked
  const [settingDueId, setSettingDueId] = useState<string | null>(null)

  const dragEnabled = canDrag(display)
  const { columns, lanes } = boardGrid(tasks, display, groupContext)
  // the scroller is state: the cells' virtualizers attach to it once it is in the DOM
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  const attachScroller = useCallback((element: HTMLDivElement | null) => {
    scrollerRef.current = element
    setScroller(element)
  }, [])
  const savedTop = useScrollMemory(scrollerRef, 'task-board')
  // the keyboard walks the cards in document order: each open lane, column by column
  const openLanes = lanes?.filter((lane) => !collapsed.includes(zoneIdOf(valuesOf(lane))))
  const { keepId, onFocus } = useVirtualTaskRows(
    openLanes
      ? openLanes.flatMap((lane) => columns.flatMap((column) => cellTasks(column, lane).map((task) => task.id)))
      : columns.flatMap((column) => column.tasks.map((task) => task.id)),
  )
  const has = (property: TaskProperty) => display.properties.includes(property)
  const statusById = new Map(statuses.map((status) => [status.id, status]))
  const projectById = new Map(projects.map((project) => [project.id, project]))
  const labelById = new Map(labels.map((label) => [label.id, label]))
  const milestoneById = new Map(groupContext.milestones.map((milestone) => [milestone.id, milestone]))
  const cycleById = new Map(groupContext.cycles.map((cycle) => [cycle.id, cycle]))
  const milestoneOf = (group: TaskGroup) => (group.field === 'milestone' && group.value ? milestoneById.get(group.value) : undefined)

  const renderCard = (task: Task, values: GroupValues) => {
    const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
    const status = statusById.get(task.statusId)
    const cardLabels = has('labels') ? task.labels.flatMap((id) => labelById.get(id) ?? []) : []
    // a missing due date or assignee is set from a quiet glyph in the header that shows on hover
    const setDue = has('due_date') && (!task.dueAt || settingDueId === task.id)
    const showDue = has('due_date') && Boolean(task.dueAt) && !setDue
    const showTop = has('status') || has('id') || Boolean(task.blocked) || has('assignee') || setDue || has('priority')
    const showProgress = has('sub_issue_progress') && (task.subIssueCount ?? 0) > 0
    const milestone = has('milestone') && task.milestoneId ? milestoneById.get(task.milestoneId) : undefined
    const cycle = has('cycle') && task.cycleId ? cycleById.get(task.cycleId) : undefined
    const showEstimate = has('estimate') && task.estimate != null
    const showMeta = showProgress || has('project') || Boolean(milestone) || Boolean(cycle) || showEstimate || showDue || has('created') || has('updated')
    return (
      <article
        data-board-card
        className={cn(
          'group/card relative flex cursor-pointer flex-col gap-[7px] rounded-md border bg-card p-2.5 transition-[translate,background-color,border-color,box-shadow,opacity] duration-150 ease-out focus-visible:ring-1 focus-visible:ring-primary/70 focus-visible:outline-none data-active:border-primary/40 data-dragging:border-dashed data-dragging:opacity-35',
          // lift only where hover is real and motion is welcome; the hover styles skip a nest target: `hover-fine` is
          // emitted after `data-*` at equal specificity, so they must be mutually exclusive (.ai/lessons.md)
          'not-data-[nest=inside]:hover-fine:hover:border-foreground/20 not-data-[nest=inside]:hover-fine:hover:bg-accent not-data-[nest=inside]:hover-fine:hover:shadow-md motion-safe:not-data-[nest=inside]:hover-fine:hover:-translate-y-px',
          // while a drop would nest into the card: tint + inset ring (the chip names the action)
          'data-[nest=inside]:border-primary/40 data-[nest=inside]:bg-primary/10 data-[nest=inside]:ring-1 data-[nest=inside]:ring-primary/40 data-[nest=inside]:ring-inset',
        )}
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
          if (event.target !== event.currentTarget) return
          if (event.key === 'Enter') onOpen(task.id)
          // J and K walk a column; the arrows cross to the next one
          if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
            event.preventDefault()
            focusAdjacentColumn(event.currentTarget, event.key === 'ArrowRight' ? 1 : -1)
          }
        }}
        {...nest.rowProps(task)}
        {...taskRowTarget(task.id)}
      >
        {/* status · id · blocked … [set due date] · assignees · priority (status, due date, assignees and priority change in place) */}
        {showTop ? (
          <div className="flex min-h-5 items-center justify-between gap-2 text-[0.6875rem] text-muted-foreground/70">
            <span className="flex min-w-0 items-center gap-1">
              {has('status') ? <StatusPicker task={task} statuses={statuses} size={12} className="-m-1 size-5" onRequestDuplicate={() => setDuplicateTask(task)} /> : null}
              {has('id') ? <span className="whitespace-nowrap tabular-nums">{task.identifier}</span> : null}
              {task.blocked ? <BlockedIndicator /> : null}
            </span>
            <span className="flex items-center gap-1">
              {setDue ? <DueDatePicker task={task} status={status} empty="icon" onOpenChange={(open) => setSettingDueId((current) => open ? task.id : current === task.id ? null : current)} className="hover-fine:opacity-0 hover-fine:group-hover/card:opacity-100 focus-within:opacity-100 has-data-popup-open:opacity-100" /> : null}
              {has('assignee') ? (
                <AssigneePicker
                  task={task}
                  users={users}
                  max={3}
                  className={cn(assignees.length === 0 && 'hover-fine:opacity-0 hover-fine:group-hover/card:opacity-100 focus-within:opacity-100 has-data-popup-open:opacity-100')}
                />
              ) : null}
              {has('priority') ? <PriorityPicker task={task} align="right" /> : null}
            </span>
          </div>
        ) : null}
        {/* the board always renders Nested as Flat: a sub-issue names its parent above its title */}
        {task.parent ? (
          <Button
            type="button"
            variant="link"
            className="-mb-1 h-auto max-w-full min-w-0 shrink gap-1 self-start p-0 text-[0.6875rem] font-normal text-muted-foreground hover:text-foreground"
            onClick={(event) => {
              event.stopPropagation()
              onOpen(task.parent!.id)
            }}
          >
            <span className="truncate">{task.parent.title || 'Untitled'}</span>
            <span aria-hidden className="shrink-0 text-muted-foreground/50">›</span>
          </Button>
        ) : null}
        <h3 className="text-[0.8125rem] leading-[1.125rem] font-medium text-foreground">{task.title || 'Untitled'}</h3>
        {cardLabels.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {cardLabels.map((label) => <LabelPill key={label.id} label={label} />)}
          </div>
        ) : null}
        {showMeta ? (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[0.6875rem] text-muted-foreground/70">
            {showProgress ? <SubIssueProgress closed={task.subIssueClosedCount ?? 0} total={task.subIssueCount ?? 0} color={completedStatusColor(statuses, task.projectId)} /> : null}
            {has('project') ? <ProjectChip project={projectById.get(task.projectId)} /> : null}
            <MilestoneChip milestone={milestone} />
            <CycleChip cycle={cycle} />
            {showEstimate ? <EstimateChip task={task} /> : null}
            {showDue ? <DueDatePicker task={task} status={status} /> : null}
            {has('created') ? <DateStamp property="created" iso={task.createdAt} /> : null}
            {has('updated') ? <DateStamp property="updated" iso={task.updatedAt} /> : null}
          </div>
        ) : null}
        {/* bottom-right, clear of the header's assignee and priority that identify the drop target */}
        {nest.nestAt?.id === task.id && nest.nestAt.zone === 'inside' ? <NestChip className="right-2 bottom-2" /> : null}
      </article>
    )
  }

  const columnHeader = (column: TaskGroup) => (
    <header className="flex h-[38px] items-center gap-[7px] px-2.5 text-xs font-semibold text-muted-foreground transition-colors duration-150 group-data-[drop-over]/col:text-primary">
      <GroupIcon group={column} context={groupContext} />
      <span className="truncate">{column.label}</span>
      <MilestoneLate milestone={milestoneOf(column)} />
      <GroupTotals tasks={column.tasks} className="ml-auto" />
    </header>
  )

  const renderZone = (props: Pick<BoardZoneProps, 'variant' | 'zone' | 'values' | 'cell'> & { header?: ReactNode }) => (
    <BoardZone
      key={props.zone}
      {...props}
      scroller={scroller}
      savedTop={savedTop}
      sizeKey={display.properties.join(',')}
      dragId={drag?.taskId ?? null}
      drop={drop}
      dragHeight={dragHeight}
      keepId={keepId}
      zoneProps={zoneProps}
      renderCard={renderCard}
    />
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
      <div ref={attachScroller} data-slot="task-board" data-virtual-scroller className="h-full overflow-auto" onFocus={onFocus}>
        <div className="relative grid min-h-full min-w-max gap-3 p-3" aria-busy={saving} style={gridStyle}>
          {columns.map((column) => {
            const values = valuesOf(column)
            return renderZone({ variant: 'column', zone: zoneIdOf(values), values, cell: column.tasks, header: columnHeader(column) })
          })}
          {picker}
        </div>
      </div>
    )
  }

  return (
    // a card brought into view (keyboard) stops below the sticky column and lane headers
    <div ref={attachScroller} data-slot="task-board" data-virtual-scroller className="h-full scroll-pt-[86px] overflow-auto" onFocus={onFocus}>
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
            {/* column headers are 12px padding + 38px tall; lane headers stick right under them. A collapsed lane is a
                drop zone itself, tinted like a list group header while a card is over it */}
            <div className="group/lane sticky top-[50px] z-10 col-span-full flex h-9 items-center border-b bg-background transition-colors duration-150 data-drop-over:bg-primary/10" {...laneDrop}>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                // sticks to the left edge while the board scrolls sideways, so the lane name stays in view
                className="sticky left-3 gap-2 px-1.5 text-xs font-semibold text-muted-foreground group-data-drop-over/lane:text-primary"
                aria-expanded={open}
                aria-label={`${open ? 'Collapse' : 'Expand'} ${lane.label}`}
                onClick={() => toggle(laneId)}
              >
                <DisclosureChevron open={open} />
                <GroupIcon group={lane} context={groupContext} />
                <span>{lane.label}</span>
                <MilestoneLate milestone={milestoneOf(lane)} />
                <GroupTotals tasks={lane.tasks} />
              </Button>
            </div>
            {open ? columns.map((column) => {
              const values = [...valuesOf(column), ...laneValues]
              return renderZone({ variant: 'cell', zone: zoneIdOf(values), values, cell: cellTasks(column, lane) })
            }) : null}
          </section>
        )
      })}
      {picker}
    </div>
    </div>
  )
}

interface BoardZoneProps {
  /** A whole column (header, cards, "No tasks" when empty), or one cell of a swim lane. */
  variant: 'column' | 'cell'
  zone: string
  values: GroupValues
  cell: Task[]
  header?: ReactNode
  scroller: HTMLDivElement | null
  savedTop: () => number
  /** The shown properties, which set a card's height. */
  sizeKey: string
  dragId: string | null
  drop: ReturnType<typeof useGroupDrop>['drop']
  dragHeight: number
  /** The card with the keyboard focus: it stays in the DOM out of view. */
  keepId: string | null
  zoneProps: ReturnType<typeof useGroupDrop>['zoneProps']
  renderCard: (task: Task, values: GroupValues) => ReactNode
}

/**
 * A card that renders again only when the board does: a scroll changes the zone's range many times a second, and
 * then only the cards that come into view render (a card with its pickers is costly).
 */
/** A column or lane header's card count, then the sum of the estimates once a card has one. */
function GroupTotals({ tasks, className }: { tasks: Task[]; className?: string }) {
  const points = estimateTotal(tasks)
  return (
    <span className={cn('flex items-center gap-2 font-normal whitespace-nowrap text-muted-foreground/70 tabular-nums', className)}>
      <span>{tasks.length}</span>
      {points === null ? null : <span title="Total estimate">{estimateShortLabel(points)}</span>}
    </span>
  )
}

const BoardCard = memo(function BoardCard({ task, values, renderCard }: { task: Task; values: GroupValues; renderCard: BoardZoneProps['renderCard'] }) {
  return renderCard(task, values)
})

/** An element's top in a scroller's content, rounded so that a sub-pixel change renders nothing. */
const topIn = (scroller: HTMLElement, element: HTMLElement) =>
  Math.round(element.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop)

/**
 * One drop zone of the board with its cards in a virtual list on the board's scroller. The list starts lower in the
 * scroller (`scrollMargin`): it is measured again when the board's content changes size (a lane above grows). A zone
 * far outside the view (a lane below, a column off to the side) mounts no cards. The dragged card stays mounted
 * (unmounting the drag source cancels the browser drag), and the drop placeholder pushes the cards below it down.
 */
function BoardZone({ variant, zone, values, cell, header, scroller, savedTop, sizeKey, dragId, drop, dragHeight, keepId, zoneProps, renderCard }: BoardZoneProps) {
  const list = useRef<HTMLDivElement>(null)
  const [margin, setMargin] = useState(0)
  const [near, setNear] = useState(false)
  // the list's top in the scroller: again when the board renders new cells (a card moved between lanes) and when the
  // board's content changes size (a card in a lane above was measured)
  useLayoutEffect(() => {
    if (scroller && list.current) setMargin(topIn(scroller, list.current))
  }, [scroller, cell])
  useLayoutEffect(() => {
    const element = list.current
    const content = scroller?.firstElementChild
    if (!scroller || !element || !content) return
    // before the first paint, so a zone out of view never mounts cards; the observer follows the scroll after
    const rect = element.getBoundingClientRect()
    const view = scroller.getBoundingClientRect()
    setNear(rect.bottom > view.top - ZONE_MARGIN && rect.top < view.bottom + ZONE_MARGIN && rect.right > view.left - ZONE_MARGIN && rect.left < view.right + ZONE_MARGIN)
    const resize = new ResizeObserver(() => setMargin(topIn(scroller, element)))
    resize.observe(content)
    const intersection = new IntersectionObserver((entries) => setNear(entries[entries.length - 1].isIntersecting), { root: scroller, rootMargin: `${ZONE_MARGIN}px` })
    intersection.observe(element)
    return () => {
      resize.disconnect()
      intersection.disconnect()
    }
  }, [scroller])
  const kept = cell.flatMap((task, index) => (task.id === keepId || task.id === dragId ? [index] : []))
  const virtualizer = useVirtualizer({
    count: cell.length,
    getScrollElement: () => scroller,
    // where the board is: the remembered position on the first mount (back from a task), else the current one (a
    // column or lane added later must not move the board)
    initialOffset: () => scroller?.scrollTop ?? savedTop(),
    scrollMargin: margin,
    gap: CARD_GAP,
    getItemKey: (index) => cell[index].id,
    estimateSize: (index) => estimatedSize(sizeKey, cell[index].id),
    measureElement: (element, entry, instance) => {
      const size = measureSize(element, entry, instance)
      const task = cell[instance.indexFromElement(element)]
      if (task && size > 0) recordSize(sizeKey, task.id, size)
      return size
    },
    overscan: 5,
    // the zones render in one batch when a scroll starts or ends, not one by one
    useFlushSync: false,
    rangeExtractor: (range) => {
      // out of view: no cards, except a column's card nearest the view, which the arrow keys move the focus to
      const indexes = near ? defaultRangeExtractor(range) : variant === 'column' && range.count > 0 ? [range.startIndex] : []
      const outside = kept.filter((index) => !indexes.includes(index))
      return outside.length > 0 ? [...indexes, ...outside].sort((a, b) => a - b) : indexes
    },
  })
  // every zone shares the scroller: correcting the scroll for one column's measured card would move all the others
  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = () => false
  const items = virtualizer.getVirtualItems()
  const measurements = virtualizer.measurementsCache
  const listHeight = virtualizer.getTotalSize()

  // the placeholder goes before the card at `slotCell` (cell index; the slot counts only the cards that can receive
  // the drop) and pushes it and every card after it down
  const dragIndex = dragId ? cell.findIndex((task) => task.id === dragId) : -1
  const others = cell.length - (dragIndex === -1 ? 0 : 1)
  const slot = drop?.zone === zone ? drop.index : null
  const slotCell = slot === null ? null : slot >= others ? cell.length : slot + (dragIndex !== -1 && dragIndex <= slot ? 1 : 0)
  const placeholderHeight = Math.max(dragHeight, 44)
  const topOf = (index: number) => measurements[index].start - margin + (slotCell !== null && index >= slotCell ? placeholderHeight + CARD_GAP : 0)
  const placeholderTop = slotCell === null ? 0 : slotCell < cell.length ? measurements[slotCell].start - margin : cell.length > 0 ? listHeight + CARD_GAP : 0
  const height = listHeight + (slotCell === null ? 0 : placeholderHeight + (cell.length > 0 ? CARD_GAP : 0))

  /** The insertion slot at the pointer, by card midpoints as they show (pushed down by the placeholder). */
  const slotAt = (clientY: number) => {
    const y = clientY - (list.current?.getBoundingClientRect().top ?? 0)
    let count = 0
    for (let index = 0; index < cell.length; index += 1) {
      if (index === dragIndex) continue
      if (y < topOf(index) + measurements[index].size / 2) break
      count += 1
    }
    return count
  }

  const cards = (
    <div ref={list} className="relative" style={{ height }}>
      {items.map((item) => (
        <div key={item.key} ref={virtualizer.measureElement} data-index={item.index} className="absolute inset-x-0" style={{ top: topOf(item.index) }}>
          <BoardCard task={cell[item.index]} values={values} renderCard={renderCard} />
        </div>
      ))}
      {/* held (a card owns the hover): hidden but keeps its space, so the cards under the pointer do not shift */}
      {slotCell !== null ? <div data-board-placeholder data-held={drop?.held || undefined} className="absolute inset-x-0 rounded-md border border-dashed border-primary/40 bg-primary/10 data-held:invisible" style={{ top: placeholderTop, height: placeholderHeight }} /> : null}
    </div>
  )
  const dropProps = zoneProps({ id: zone, values, tasks: cell, itemsShown: true, slotAt })

  if (variant === 'cell') {
    return (
      <div data-board-cell={zone} className="my-2 min-h-[72px] min-w-0 rounded-md bg-muted/40 p-[7px] transition-shadow data-drop-over:ring-1 data-drop-over:ring-primary/40 data-drop-over:ring-inset" {...dropProps}>
        {cards}
      </div>
    )
  }
  return (
    <section className="group/col min-w-0 rounded-md bg-muted/40 transition-shadow data-drop-over:ring-1 data-drop-over:ring-primary/40 data-drop-over:ring-inset" {...dropProps}>
      {header}
      <div className="min-h-[120px] px-[7px] pb-[7px]">
        {cards}
        {others === 0 && drop?.zone !== zone ? <div className="flex h-[72px] items-center justify-center text-xs text-muted-foreground/70">No tasks</div> : null}
      </div>
    </section>
  )
}
