import { useTaskTarget, useVirtualTaskRows } from '@/shortcuts/taskTarget'
import { useCommand } from '@/shortcuts/useCommand'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { defaultRangeExtractor, useVirtualizer, type VirtualItem } from '@tanstack/react-virtual'
import { Calendar, Copy, Danger, Flag, Hierarchy2, LinkBroken, Loader, Add as Plus, RecordCircle, TaskSquare as SquareCheck, Tag, UserAdd, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { DatePicker, type DueRange } from '@/components/common/DatePicker'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { EmptyState } from '@/components/common/EmptyState'
import { PriorityIcon } from './PriorityIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { BulkItem, LabelRecord } from '@/api/generated/types.gen'
import { assigneeToggleUpdates, dueUpdates, labelToggleUpdates, priorityUpdates, statusUpdates } from '@/features/tasks/bulkUpdates'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES, useBulkTasks } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { DisclosureChevron, GroupIcon } from '@/features/views/components/GroupIcon'
import { groupTasks, type GroupContext, type TaskGroup } from '@/features/views/grouping'
import { canDrag, listSections, ownGroupValues, valuesOf, zoneIdOf, type GroupValues } from '@/features/views/layoutGroups'
import { useCollapsedGroups } from '@/features/views/useCollapsedGroups'
import { useCollapsedTasks } from '@/features/views/useCollapsedTasks'
import { buildTaskTree, descendantIds, flattenTree, subtreeSize, type TreeRow } from '@/features/views/taskTree'
import { useGroupDrop } from '@/features/views/useGroupDrop'
import { useNestDrop } from '@/features/views/useNestDrop'
import type { DisplayOptions } from '@/features/views/viewState'
import { TaskRow } from './TaskRow'
import { LabelPill } from './TaskLabels'
import { pickerTitle } from '@/features/tasks/relationsLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { useParentActions } from '@/features/tasks/useParentActions'
import { parentPickerTitle } from '@/features/tasks/subIssuesLib'
import { TaskPickerDialog } from './TaskPickerDialog'
import { useSlowPending } from '@/lib/useDebouncedValue'

export interface TaskListProps {
  tasks: Task[]
  users: User[]
  labels: LabelRecord[]
  statuses: TaskStatusDef[]
  projects: Project[]
  display: DisplayOptions
  groupContext: GroupContext
  /** Page scope for collapsed groups: `all`, `project:<id>`, `preset:<name>`, `view:<id>`. Key the list by it. */
  collapseScope: string
  onOpen: (taskId: string) => void
  /** + in a group header: the group (and sub-group) values the new task takes. */
  onAdd: (values: GroupValues) => void
}

/** The height of a line before it is measured. */
const LINE_HEIGHT = { group: 36, sub: 32, row: 40 }

/** A group or sub-group: its header's data, and a drop zone when it holds the rows itself. */
type Zone = { id: string; values: GroupValues; tasks: Task[]; itemsShown: boolean }
/** One line of the virtual list. `keep`: the headers above it, which stay mounted while it is the first line in view (they stick). */
type Line = { key: string; zone: Zone; keep: number[] } & (
  | { kind: 'group' | 'sub'; group: TaskGroup }
  | { kind: 'row'; row: TreeRow })
/** A `<section>` over the lines `first`…`last`: a header (unless ungrouped), then rows or sub-group sections. */
type Block = { key: string; first: number; last: number; zone: Zone; drop: boolean; className?: string; blocks?: Block[] }

/**
 * Grouped task list: collapsible group and sub-group headers; rows move between groups by drag and drop.
 * Only the lines near the view are in the DOM (TanStack Virtual); the list has its own scroll.
 */
export function TaskList({ tasks, users, labels, statuses, projects, display, groupContext, collapseScope, onOpen, onAdd }: TaskListProps) {
  const { workspace } = useWorkspace()
  const [collapsed, toggle] = useCollapsedGroups(`orbit:task_list_collapsed:${workspace.id}:${collapseScope}`)
  // shared with the keyboard: X selects the focused row and task commands act on the selection
  const { selectedIds: selected, setSelected } = useTaskTarget()
  // a list for another page starts without a selection
  useEffect(() => () => setSelected([]), [setSelected])
  const duplicates = useDuplicateActions(workspace.id)
  // tasks waiting for a canonical task: one (row menu, drop on the Duplicate group) or the bulk selection
  const [duplicatePicker, setDuplicatePicker] = useState<Task[] | null>(null)
  const parentActions = useParentActions(workspace.id)
  // tasks waiting for a parent: the bulk selection
  const [parentPicker, setParentPicker] = useState<Task[] | null>(null)
  const manual = display.order_by === 'manual'

  const nested = display.sub_issues === 'nested'
  // built in every mode: the bulk "Set parent…" picker excludes descendants from it
  const tree = useMemo(() => buildTaskTree(tasks), [tasks])
  const taskTree = useCollapsedTasks(workspace.id)
  const { drag, drop, startDrag, endDrag, clearDrop, zoneProps } = useGroupDrop({
    tasks,
    manual,
    groupContext,
    onDuplicate: (task) => setDuplicatePicker([task]),
    // nested: a group zone is the root level, so a sub-issue dropped there leaves its parent
    detach: nested ? (task) => tree.nested.has(task.id) : undefined,
    onDetached: (task, records, response) => parentActions.announce([task], null, records, response),
  })
  const nest = useNestDrop({
    tasks,
    tree: nested ? tree : null,
    manual,
    dragId: drag?.taskId ?? null,
    onTakeOver: clearDrop,
    endDrag,
    onNest: (plan, dragId) => {
      const moving = tasks.find((task) => task.id === dragId)
      const parent = tasks.find((task) => task.id === plan.parentId)
      if (!moving || !parent) return
      // the new sub-issue stays in view under its parent
      taskTree.expand(parent.id)
      void parentActions.setParent([moving], { id: parent.id, identifier: parent.identifier }, { placement: plan.placement })
    },
  })
  // a subtree stays in its root's group (spec §7.2), so only roots are grouped; counts include nested rows
  const rowCount = (zoneTasks: Task[]) => nested ? zoneTasks.reduce((total, task) => total + 1 + subtreeSize(tree, task.id), 0) : zoneTasks.length

  const dragEnabled = canDrag(display)
  const sections = listSections(nested ? tree.roots : tasks, display, groupContext)
  const projectById = new Map(projects.map((project) => [project.id, project]))
  const statusOptions = groupTasks([], 'status', { ...groupContext, showEmpty: true })
    .filter((group) => !group.value?.startsWith('duplicate:'))

  const lines: Line[] = []
  const addRows = (zone: Zone, keep: number[]) => {
    const rows: TreeRow[] = nested
      ? flattenTree(tree, zone.tasks, taskTree.collapsed)
      : zone.tasks.map((task) => ({ task, depth: 0, indent: 0, hasChildren: false }))
    // a task with several labels or assignees has a row in each of their groups
    for (const row of rows) lines.push({ kind: 'row', key: `${zone.id}:${row.task.id}`, zone, keep, row })
  }
  const blocks = sections.map(({ group, subGroups }): Block => {
    const values = valuesOf(group)
    const id = zoneIdOf(values)
    const first = lines.length
    if (group.field === 'none') {
      const zone = { id, values, tasks: group.tasks, itemsShown: true }
      addRows(zone, [])
      return { key: id, first, last: lines.length - 1, zone, drop: true }
    }
    const open = !collapsed.includes(id)
    const zone = { id, values, tasks: group.tasks, itemsShown: open }
    lines.push({ kind: 'group', key: `group:${id}`, zone, keep: [], group })
    // a collapsed group hides its sub-groups, so the group itself takes drops: only the group field changes
    // (a hidden sub-group value must never be written) and the task goes to the group's end
    const subBlocks = open ? subGroups?.map((sub): Block => {
      const subValues = valuesOf(group, sub)
      const subId = zoneIdOf(subValues)
      const subFirst = lines.length
      const subZone = { id: subId, values: subValues, tasks: sub.tasks, itemsShown: !collapsed.includes(subId) }
      lines.push({ kind: 'sub', key: `sub:${subId}`, zone: subZone, keep: [first], group: sub })
      if (subZone.itemsShown) addRows(subZone, [first, subFirst])
      return { key: sub.key, first: subFirst, last: lines.length - 1, zone: subZone, drop: true, className: 'group/sub' }
    }) : undefined
    if (open && !subGroups) addRows(zone, [first])
    return { key: group.key, first, last: lines.length - 1, zone, drop: !subBlocks, className: 'group/section', blocks: subBlocks }
  })

  const scroller = useRef<HTMLDivElement>(null)
  const content = useRef<HTMLDivElement>(null)
  const { keepId, onFocus } = useVirtualTaskRows(lines.flatMap((line) => (line.kind === 'row' ? [line.row.task.id] : [])))
  // rows that stay in the DOM out of view: the dragged row (see `renderRow`) and the row with the focus
  const kept = lines.flatMap((line, index) => (line.kind === 'row' && (line.row.task.id === keepId || line.row.task.id === drag?.taskId) ? [index] : []))
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => scroller.current,
    getItemKey: (index) => lines[index].key,
    estimateSize: (index) => LINE_HEIGHT[lines[index].kind],
    overscan: 10,
    rangeExtractor: (range) => {
      const indexes = defaultRangeExtractor(range)
      const outside = [...(lines[range.startIndex]?.keep ?? []), ...kept].filter((index) => index < indexes[0] || index > indexes[indexes.length - 1])
      return outside.length > 0 ? [...new Set([...indexes, ...outside])].sort((a, b) => a - b) : indexes
    },
  })
  const mounted = virtualizer.getVirtualItems()
  const measurements = virtualizer.measurementsCache

  /** The insertion slot among a zone's root rows (a subtree moves with its root) at the pointer, by row midpoints. */
  const slotAt = (block: Block, clientY: number) => {
    const y = clientY - (content.current?.getBoundingClientRect().top ?? 0)
    let slot = 0
    for (let index = block.first; index <= block.last; index += 1) {
      const line = lines[index]
      if (line.kind !== 'row' || line.row.depth > 0 || line.row.task.id === drag?.taskId) continue
      if (y < measurements[index].start + measurements[index].size / 2) break
      slot += 1
    }
    return slot
  }

  const toggleSelect = (taskId: string) =>
    setSelected((prev) => (prev.includes(taskId) ? prev.filter((id) => id !== taskId) : [...prev, taskId]))

  const selectedTasks = tasks.filter((t) => selected.includes(t.id))

  // with "Show empty groups" the empty group headers stay, so a task can still be added to a group
  if (tasks.length === 0 && sections.every((section) => section.group.field === 'none')) {
    return (
      <EmptyState
        icon={SquareCheck}
        title="No tasks found"
        description="No tasks match the current filters. Try clearing a filter or create a new task."
      />
    )
  }

  /**
   * Group zone props with one drop indicator at a time: while an insertion line marks the slot (manual order, rows in
   * view), the group shows no outline or header tint, as in Linear. Drops with no line (another group without manual
   * order, a collapsed or empty group) keep the outline.
   */
  const dropZoneProps = (zone: Parameters<typeof zoneProps>[0]) => {
    const props = zoneProps(zone)
    const lineShown = drop?.zone === zone.id && drop.index !== null && zone.itemsShown && zone.tasks.some((task) => task.id !== drag?.taskId)
    return lineShown ? { ...props, 'data-drop-over': undefined } : props
  }

  const renderRow = (line: Extract<Line, { kind: 'row' }>, item: VirtualItem, origin: number, lastRowId: string | undefined) => {
    const { task, depth, hasChildren } = line.row
    const { zone } = line
    const index = drop?.zone === zone.id ? drop.index : null
    // the dragged row stays mounted (faded): unmounting the drag source cancels the browser drag
    const others = index === null ? [] : zone.tasks.filter((other) => other.id !== drag?.taskId)
    const slot = depth === 0 ? others.indexOf(task) : -1
    const dropEdge = index === null ? null
      : slot !== -1 && slot === index ? 'top'
        : index === others.length && task.id === lastRowId ? 'bottom' : null
    // the edge of a sub-issue: the task becomes its sibling, shown with the same line
    const nestEdge = nest.nestAt?.id === task.id && nest.nestAt.zone !== 'inside' ? (nest.nestAt.zone === 'before' ? 'top' : 'bottom') : null
    return (
      <div key={line.key} ref={virtualizer.measureElement} data-index={item.index} className="absolute inset-x-0" style={{ top: item.start - origin }}>
        <TaskRow
          task={task}
          labels={labels}
          statuses={statuses}
          users={users}
          assignees={users.filter((u) => task.assigneeIds.includes(u.id))}
          project={projectById.get(task.projectId)}
          properties={display.properties}
          selected={selected.includes(task.id)}
          dragging={task.id === drag?.taskId}
          draggable={dragEnabled}
          dropEdge={nestEdge ?? dropEdge}
          onOpen={onOpen}
          onToggleSelect={toggleSelect}
          // a nested row shows in its root's group but moves from its own (status, label, assignee…)
          onDragStart={(taskId) => startDrag(taskId, depth > 0 ? ownGroupValues(task, zone.values, groupContext) : zone.values)}
          onDragEnd={endDrag}
          onRequestDuplicate={(rowTask) => setDuplicatePicker([rowTask])}
          tree={nested ? { depth, hasChildren, expanded: !taskTree.collapsed.has(task.id), onToggle: () => taskTree.toggle(task.id) } : null}
          showParent={!nested || !tree.nested.has(task.id)}
          nest={nest.rowProps(task)}
        />
      </div>
    )
  }

  const renderHeader = (line: Extract<Line, { kind: 'group' | 'sub' }>, index: number) => {
    const { group, kind } = line
    const { id: zone, values } = line.zone
    const isCollapsed = collapsed.includes(zone)
    // inside the Duplicate status (group or sub-group) a new task would need a canonical task first
    const canAdd = !values.some((value) => value.field === 'status' && value.value?.startsWith('duplicate:'))
    return (
      <div
        ref={virtualizer.measureElement}
        data-index={index}
        data-slot="group-header"
        data-level={kind}
        className={cn(
          'group/hdr sticky flex items-center gap-2 border-b pr-2 text-xs text-muted-foreground transition-colors duration-150',
          // sub-group headers stick right under their group header and sit one indent step in
          kind === 'group'
            ? 'top-0 z-[5] h-9 bg-card pl-1.5 font-semibold group-data-drop-over/section:bg-primary/10 group-data-drop-over/section:text-primary'
            : 'top-9 z-[4] h-8 border-border/60 bg-background pl-6 font-medium group-data-drop-over/sub:bg-primary/10 group-data-drop-over/sub:text-primary',
        )}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="size-5 text-muted-foreground"
          aria-expanded={!isCollapsed}
          aria-label={`${isCollapsed ? 'Expand' : 'Collapse'} ${group.label}`}
          onClick={() => toggle(zone)}
        >
          <DisclosureChevron open={!isCollapsed} />
        </Button>
        <GroupIcon group={group} context={groupContext} />
        <span className="truncate">{group.label}</span>
        <span className="font-normal text-muted-foreground/70 tabular-nums">{rowCount(group.tasks)}</span>
        <div className="flex-1" />
        {canAdd ? (
          <Tip label="New task">
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              // revealed on hover only where hover is real (fine pointer); touch always shows it
              className="text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/hdr:opacity-100 focus-visible:opacity-100"
              aria-label={`New task in ${group.label}`}
              onClick={() => onAdd(values)}
            >
              <Plus />
            </Button>
          </Tip>
        ) : null}
      </div>
    )
  }

  /**
   * A section at its place in the list, with its mounted lines. The header is in the flow and sticks inside the
   * section; rows and sub-group sections are placed from the measurements (`origin` = the top of the parent).
   */
  const renderBlock = (block: Block, origin: number): ReactNode => {
    const shown = mounted.filter((item) => item.index >= block.first && item.index <= block.last)
    if (shown.length === 0) return null
    const top = measurements[block.first].start
    const header = lines[block.first]
    // the end-of-zone line sits under the zone's last row, which may be a sub-issue of the last root
    const lastRow = drop?.zone === block.zone.id && block.drop ? lines.slice(block.first, block.last + 1).findLast((line) => line.kind === 'row' && line.row.task.id !== drag?.taskId) : undefined
    const lastRowId = lastRow?.kind === 'row' ? lastRow.row.task.id : undefined
    const Section = block.drop ? DropZone : 'section'
    return (
      <Section
        key={block.key}
        className={cn('absolute inset-x-0', block.className)}
        style={{ top: top - origin, height: measurements[block.last].end - top }}
        {...(block.drop ? dropZoneProps({ ...block.zone, slotAt: (clientY) => slotAt(block, clientY) }) : null)}
      >
        {header.kind !== 'row' && shown[0].index === block.first ? renderHeader(header, block.first) : null}
        {block.blocks
          ? block.blocks.map((child) => renderBlock(child, top))
          : shown.map((item) => {
            const line = lines[item.index]
            return line.kind === 'row' ? renderRow(line, item, top, lastRowId) : null
          })}
      </Section>
    )
  }

  return (
    <>
      <div
        ref={scroller}
        data-slot="task-list"
        data-virtual-scroller
        className="h-full overflow-y-auto"
        onFocus={onFocus}
      >
        <div ref={content} className="relative" style={{ height: virtualizer.getTotalSize() }}>
          {blocks.map((block) => renderBlock(block, 0))}
        </div>
      </div>
      {selectedTasks.length > 0 ? (
        <BulkBar
          tasks={selectedTasks}
          users={users}
          statuses={statuses}
          statusOptions={statusOptions}
          groupContext={groupContext}
          labels={labels}
          onClear={() => setSelected([])}
          onMarkDuplicate={() => setDuplicatePicker(selectedTasks)}
          onSetParent={() => setParentPicker(selectedTasks)}
          onRemoveParent={() => {
            void parentActions.setParent(selectedTasks, null)
            setSelected([])
          }}
        />
      ) : null}
      {duplicatePicker ? (
        <TaskPickerDialog
          open
          onOpenChange={(open) => { if (!open) setDuplicatePicker(null) }}
          title={pickerTitle('duplicate', duplicatePicker.length === 1 ? duplicatePicker[0]!.identifier : duplicatePicker.length)}
          statuses={statuses}
          excludeIds={duplicatePicker.map((item) => item.id)}
          excludeDuplicates
          onSelect={(target) => {
            if (duplicatePicker.length === 1) {
              void duplicates.markOne(duplicatePicker[0]!, target)
            } else {
              void duplicates.markMany(duplicatePicker, target)
              setSelected([])
            }
          }}
        />
      ) : null}
      {parentPicker ? (
        <TaskPickerDialog
          open
          onOpenChange={(open) => { if (!open) setParentPicker(null) }}
          title={parentPickerTitle(parentPicker.length === 1 ? parentPicker[0]!.identifier : parentPicker.length)}
          statuses={statuses}
          // loaded descendants only; a deeper one is refused by the server (parent_cycle) and explained in a toast
          excludeIds={parentPicker.flatMap((item) => [item.id, ...descendantIds(tree, item.id)])}
          onSelect={(target) => {
            void parentActions.setParent(parentPicker, { id: target.id, identifier: target.identifier })
            setSelected([])
          }}
        />
      ) : null}
    </>
  )
}

/** Floating toolbar for the selected rows: bulk status, priority, assignees, labels and due dates. */
function BulkBar({
  tasks,
  users,
  statuses,
  statusOptions,
  groupContext,
  labels,
  onClear,
  onMarkDuplicate,
  onSetParent,
  onRemoveParent,
}: {
  tasks: Task[]
  users: User[]
  statuses: TaskStatusDef[]
  /** Status groups (every key, Duplicate excluded) from `groupTasks`. */
  statusOptions: TaskGroup[]
  groupContext: GroupContext
  labels: LabelRecord[]
  onClear: () => void
  onMarkDuplicate: () => void
  onSetParent: () => void
  /** Shown only while a selected task has a parent. */
  onRemoveParent: () => void
}) {
  const { workspace } = useWorkspace()
  const bulkTasks = useBulkTasks(workspace.id)
  const bulkSlow = useSlowPending(bulkTasks.isPending)
  const limitError = bulkTasks.error instanceof BulkTaskLimitError ? bulkTasks.error : null

  // Esc clears the selection, unless it belongs to a field, an open menu or a dialog
  useCommand('list.clearSelection', onClear)

  const mutate = (updates: BulkItem[]) => {
    if (updates.length > 0) bulkTasks.mutate(updates)
  }
  const bulkStatus = (key: string | null) => mutate(statusUpdates(tasks, statuses, key))
  const bulkPriority = (priority: Task['priority']) => mutate(priorityUpdates(tasks, priority))
  const bulkAssign = (userId: string) => mutate(assigneeToggleUpdates(tasks, userId))
  const bulkLabel = (labelId: string) => mutate(labelToggleUpdates(tasks, labelId))

  // the picker starts from the shared due range, or empty when the selection disagrees
  const [first] = tasks
  const sameDue = tasks.every((task) => task.dueAt === first.dueAt && (task.dueStartAt ?? null) === (first.dueStartAt ?? null))
  const [dueOpen, setDueOpen] = useState(false)
  // picks collect while the picker is open (a day, then the range's last day, then a time) and are written once,
  // in one bulk request, when it closes; Clear writes at once
  const [duePick, setDuePick] = useState<DueRange | null>(null)
  const bulkDue = (start: string | null, end: string | null) => {
    mutate(dueUpdates(tasks, start, end))
    setDuePick(null)
    setDueOpen(false)
  }
  const toggleDue = (open: boolean) => {
    if (!open && duePick) bulkDue(duePick.start, duePick.end)
    else setDueOpen(open)
  }

  return (
    // outer layer centres the bar, so the enter animation can own the bar's transform
    <div className="pointer-events-none absolute inset-x-0 bottom-4 z-40 flex justify-center px-6 max-[899px]:bottom-2.5 max-[899px]:px-2">
      <div
        className="pointer-events-auto flex max-w-full items-center gap-0.5 overflow-x-auto rounded-xl border bg-popover p-1 text-popover-foreground shadow-[0_12px_32px_-8px_rgb(0_0_0/0.35),0_2px_6px_-2px_rgb(0_0_0/0.18)] duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] animate-in fade-in-0 slide-in-from-bottom-2 zoom-in-97 max-[899px]:w-full [scrollbar-width:none] dark:ring-1 dark:ring-white/5"
        role="toolbar"
        aria-label="Selected tasks"
      >
        <div className="flex h-8 shrink-0 items-center rounded-lg bg-primary/12 pl-3 text-primary dark:bg-primary/25 dark:text-primary-foreground">
          <span className="text-xs font-semibold whitespace-nowrap tabular-nums">{tasks.length} selected</span>
          <Tip label="Clear selection (Esc)">
            <Button
              variant="ghost"
              size="icon-xs"
              className="mx-1 text-current opacity-70 hover:bg-primary/15 hover:text-current hover:opacity-100 dark:hover:bg-white/10"
              aria-label="Clear selection"
              onClick={onClear}
            >
              <X />
            </Button>
          </Tip>
        </div>
        <div className="mx-1 h-4 w-px shrink-0 bg-border" aria-hidden />
        <DropdownMenu>
          <DropdownMenuTrigger render={<BulkAction icon={<RecordCircle aria-hidden />} label="Status" />} />
          <DropdownMenuContent side="top" className="w-auto min-w-45">
            {statusOptions.map((group) => (
              <DropdownMenuItem key={group.key} onClick={() => bulkStatus(group.value)}>
                <GroupIcon group={group} context={groupContext} />
                {group.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger render={<BulkAction icon={<Flag aria-hidden />} label="Priority" />} />
          <DropdownMenuContent side="top" className="w-auto min-w-45">
            {PRIORITY_ORDER.map((priority) => (
              <DropdownMenuItem key={priority} onClick={() => bulkPriority(priority)}>
                <PriorityIcon priority={priority} />
                {PRIORITY_LABEL[priority]}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger render={<BulkAction icon={<UserAdd aria-hidden />} label="Assignee" />} />
          <DropdownMenuContent side="top" align="end" className="w-auto min-w-45">
            {users.map((u) => {
              const everyone = tasks.every((t) => t.assigneeIds.includes(u.id))
              return (
                <DropdownMenuCheckboxItem key={u.id} checked={everyone} closeOnClick onCheckedChange={() => bulkAssign(u.id)}>
                  <UserAvatar user={u} size={16} />
                  {u.name}
                </DropdownMenuCheckboxItem>
              )
            })}
          </DropdownMenuContent>
        </DropdownMenu>
        {/* label toggles keep the menu open (closeOnClick defaults to false on checkbox items) so several can be flipped in a row */}
        <DropdownMenu>
          <DropdownMenuTrigger render={<BulkAction icon={<Tag aria-hidden />} label="Labels" />} />
          <DropdownMenuContent side="top" align="end" className="w-auto min-w-45">
            {labels.map((label) => {
              const everyone = tasks.every((t) => t.labels.includes(label.id))
              return (
                <DropdownMenuCheckboxItem key={label.id} checked={everyone} onCheckedChange={() => bulkLabel(label.id)}>
                  <LabelPill label={label} />
                </DropdownMenuCheckboxItem>
              )
            })}
          </DropdownMenuContent>
        </DropdownMenu>
        <Popover open={dueOpen} onOpenChange={toggleDue}>
          <PopoverTrigger render={<BulkAction icon={<Calendar aria-hidden />} label="Due date" />} />
          <PopoverContent side="top" align="end" className="w-auto gap-0 p-0">
            <DatePicker
              startValue={sameDue ? first.dueStartAt ?? null : null}
              value={sameDue ? first.dueAt : null}
              clearable={tasks.some((task) => task.dueAt)}
              onClear={() => bulkDue(null, null)}
              onChange={setDuePick}
            />
          </PopoverContent>
        </Popover>
        <BulkAction icon={<Copy aria-hidden />} label="Duplicate" aria-label="Mark as duplicate…" title="Mark as duplicate…" onClick={onMarkDuplicate} />
        <BulkAction icon={<Hierarchy2 aria-hidden />} label="Set parent" aria-label="Set parent…" title="Set parent…" onClick={onSetParent} />
        {tasks.some((task) => task.parentTaskId) ? (
          <BulkAction icon={<LinkBroken aria-hidden />} label="Remove parent" aria-label="Remove parent" title="Remove parent" onClick={onRemoveParent} />
        ) : null}
        {bulkSlow ? (
          <span role="status" className="flex shrink-0 items-center gap-1.5 px-2 text-xs whitespace-nowrap text-muted-foreground">
            <Loader aria-hidden className="size-3.5 animate-spin" />
            Updating…
          </span>
        ) : null}
        {bulkTasks.isError ? (
          <span role="alert" className="flex min-w-0 items-center gap-1.5 rounded-lg bg-destructive/10 py-1 pr-1 pl-2 text-xs text-destructive">
            <Danger aria-hidden className="size-3.5 shrink-0" />
            {limitError ? `This update includes ${limitError.count} tasks. Select ${MAX_BULK_TASK_UPDATES} or fewer and try again.` : <>Bulk update failed. <Button variant="destructive" size="xs" onClick={bulkTasks.retry}>Retry</Button></>}
          </span>
        ) : null}
      </div>
    </div>
  )
}

/** A drop zone (group or sub-group section); the ring marks the zone under the pointer. */
function DropZone({ className, ...props }: React.ComponentProps<'section'>) {
  return <section data-slot="drop-zone" className={cn('data-drop-over:ring-1 data-drop-over:ring-primary/30 data-drop-over:ring-inset', className)} {...props} />
}

/** Bulk bar action: icon + label. Below 640px only the icon shows; the label stays the accessible name. */
function BulkAction({ icon, label, title, className, ...props }: React.ComponentProps<typeof Button> & { icon: React.ReactNode; label: string }) {
  return (
    <Tip label={title}>
      <Button variant="ghost" className={cn('shrink-0 text-[13px] text-muted-foreground', className)} {...props}>
        {icon}
        <span className="max-sm:sr-only">{label}</span>
      </Button>
    </Tip>
  )
}
