import { useEffect, useState, type DragEvent } from 'react'
import { ChevronRight, Copy, Danger, Flag, Loader, Add as Plus, RecordCircle, TaskSquare as SquareCheck, Tag, UserAdd, Xmark as X } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '@/components/common/UserAvatar'
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
import type { LabelRecord } from '@/api/generated/types.gen'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES, useBulkTasks, useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { resolveStatusId } from '@/features/tasks/tasksLib'
import { GroupIcon } from '@/features/views/components/GroupIcon'
import { groupTasks, type GroupContext, type TaskGroup } from '@/features/views/grouping'
import { acceptsDrop, canDrag, listSections, placementUpdates, planDrop, valuesOf, zoneIdOf, type GroupValues } from '@/features/views/layoutGroups'
import { reportMoveError } from '@/features/views/moveErrors'
import { useCollapsedGroups } from '@/features/views/useCollapsedGroups'
import type { DisplayOptions } from '@/features/views/viewState'
import { TaskRow } from './TaskRow'
import { pickerTitle } from '@/features/tasks/relationsLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { TaskPickerDialog } from './TaskPickerDialog'

const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`
/** Multi-select rows keep room on the right for the checked indicator. */
const CHECK_OPTION =
  `group min-h-8 cursor-pointer gap-2 py-1.5 pr-8 pl-2 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-checked:bg-accent data-checked:font-medium`
const PILL = 'inline-flex h-[22px] items-center gap-1.5 overflow-visible rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const BULK_BTN = 'shrink-0 gap-1.5 px-2 text-[13px] font-medium text-muted-foreground hover:text-foreground aria-expanded:text-foreground'
const BULK_ICON = 'size-3.5 opacity-80'
/** Below 640px the actions collapse to icons; the label stays as the accessible name. */
const BULK_LABEL = 'max-sm:sr-only'

/** Drop zones: the innermost group section. The ring marks the zone under the pointer. */
const ZONE = 'data-[drop-over]:ring-1 data-[drop-over]:ring-primary/30 data-[drop-over]:ring-inset'
const GROUP_HEADER =
  'group/hdr sticky top-0 z-[5] flex h-9 items-center gap-2 border-b border-border bg-card pr-2 pl-1.5 text-xs font-semibold text-muted-foreground transition-colors duration-150 group-data-[drop-over]/section:bg-primary/10 group-data-[drop-over]/section:text-primary'
/** Sub-group headers stick right under their group header and sit one indent step in. */
const SUB_HEADER =
  'group/hdr sticky top-9 z-[4] flex h-8 items-center gap-2 border-b border-border/60 bg-background pr-2 pl-6 text-xs font-medium text-muted-foreground transition-colors duration-150 group-data-[drop-over]/sub:bg-primary/10 group-data-[drop-over]/sub:text-primary'
/** Header buttons: hover only under a fine pointer; a press scales to 0.97 instead of the default 1px nudge. */
const PRESS = 'duration-150 ease-out active:not-aria-[haspopup]:translate-y-0 active:scale-[0.97] motion-reduce:active:scale-100'
const TOGGLE = `size-5 rounded-md border-0 text-muted-foreground/70 transition-[color,background-color,scale] hover-fine:hover:bg-accent hover-fine:hover:text-foreground dark:hover-fine:hover:bg-accent ${PRESS}`
/** 150ms rotation, strong ease-out; content never animates its height. Reduced motion: no rotation. */
const CHEVRON = 'size-3 transition-transform duration-150 ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:transition-none'
/** Revealed on hover only where hover is real (fine pointer); touch always shows it. */
const ADD =
  `size-6 rounded-md border-0 text-muted-foreground/70 transition-[opacity,background-color,color,scale] hover-fine:opacity-0 hover-fine:group-hover/hdr:opacity-100 hover-fine:hover:bg-accent hover-fine:hover:text-foreground focus-visible:opacity-100 dark:hover-fine:hover:bg-accent ${PRESS}`

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

type ListDrag = { taskId: string; from: GroupValues }
/** `index` = insertion slot among the zone's other rows; null when the order is not manual. */
type ListDrop = { zone: string; index: number | null }
/**
 * Where a drop lands: the innermost group section on screen. A collapsed group with sub-groups is one zone
 * with the group's values only. `rowsShown` is false when the zone's rows are not on screen (collapsed).
 */
type DropZone = { id: string; values: GroupValues; tasks: Task[]; rowsShown: boolean }

/** Slot among the zone's rows (not counting the dragged one) at the pointer, by row midpoints. */
function indexAt(zone: HTMLElement, clientY: number) {
  const rows = Array.from(zone.querySelectorAll<HTMLElement>('[data-task-row]:not([data-dragging])'))
  const below = rows.findIndex((row) => {
    const rect = row.getBoundingClientRect()
    return clientY < rect.top + rect.height / 2
  })
  return below === -1 ? rows.length : below
}

/**
 * Drag and drop between group zones: the drag state, the props for each zone, and the drop → write mapping.
 * Kept free of list markup so the board can share it. Errors go through `reportMoveError`.
 */
function useGroupDrop({ tasks, manual, groupContext, onDuplicate }: {
  tasks: Task[]
  manual: boolean
  groupContext: GroupContext
  /** A drop on the Duplicate status: the caller asks for the canonical task. */
  onDuplicate: (task: Task) => void
}) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const moveTasks = useBulkTasks(workspace.id)
  const [drag, setDrag] = useState<ListDrag | null>(null)
  const [drop, setDrop] = useState<ListDrop | null>(null)

  const startDrag = (taskId: string, from: GroupValues) => setDrag({ taskId, from })
  const endDrag = () => {
    setDrag(null)
    setDrop(null)
  }

  const dropInto = (current: ListDrag, zone: DropZone, index: number | null) => {
    const task = tasks.find((item) => item.id === current.taskId)
    if (!task) return
    if (zoneIdOf(current.from) === zone.id) {
      if (index === null) return
      const updates = placementUpdates(task, zone.tasks, index)
      if (updates.length > 0) moveTasks.mutate(updates, { onError: reportMoveError })
      return
    }
    const plan = planDrop(task, current.from, zone.values, groupContext)
    if (plan.kind === 'duplicate') onDuplicate(task)
    else if (plan.kind === 'error') toast.error(plan.message)
    else if (plan.kind === 'update') {
      if (index !== null) moveTasks.mutate(placementUpdates(task, zone.tasks, index, plan.patch), { onError: reportMoveError })
      else updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, ...plan.patch } }, { onError: reportMoveError })
    }
  }

  const zoneProps = (zone: DropZone) => {
    const indexFor = (element: HTMLElement, clientY: number) => {
      if (!manual) return null
      // no rows to aim at: the task goes to the zone's end
      if (!zone.rowsShown) return zone.tasks.filter((task) => task.id !== drag?.taskId).length
      return indexAt(element, clientY)
    }
    return {
      'data-drop-over': drop?.zone === zone.id || undefined,
      onDragOver: (event: DragEvent<HTMLElement>) => {
        if (!drag || !acceptsDrop(drag.from, zone.values)) return
        // same zone without manual order: nothing would change, so no drop target
        if (!manual && zoneIdOf(drag.from) === zone.id) return
        event.preventDefault()
        event.stopPropagation()
        event.dataTransfer.dropEffect = 'move'
        const index = indexFor(event.currentTarget, event.clientY)
        if (drop?.zone !== zone.id || drop.index !== index) setDrop({ zone: zone.id, index })
      },
      onDragLeave: (event: DragEvent<HTMLElement>) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null) && drop?.zone === zone.id) setDrop(null)
      },
      onDrop: (event: DragEvent<HTMLElement>) => {
        event.preventDefault()
        event.stopPropagation()
        if (drag) dropInto(drag, zone, indexFor(event.currentTarget, event.clientY))
        endDrag()
      },
    }
  }

  return { drag, drop, startDrag, endDrag, zoneProps }
}

/** Grouped task list: collapsible group and sub-group headers; rows move between groups by drag and drop. */
export function TaskList({ tasks, users, labels, statuses, projects, display, groupContext, collapseScope, onOpen, onAdd }: TaskListProps) {
  const { workspace } = useWorkspace()
  const [collapsed, toggle] = useCollapsedGroups(`orbit:task_list_collapsed:${workspace.id}:${collapseScope}`)
  const [selected, setSelected] = useState<string[]>([])
  const duplicates = useDuplicateActions(workspace.id)
  // tasks waiting for a canonical task: one (row menu, drop on the Duplicate group) or the bulk selection
  const [duplicatePicker, setDuplicatePicker] = useState<Task[] | null>(null)
  const { drag, drop, startDrag, endDrag, zoneProps } = useGroupDrop({
    tasks,
    manual: display.order_by === 'manual',
    groupContext,
    onDuplicate: (task) => setDuplicatePicker([task]),
  })

  const dragEnabled = canDrag(display)
  const sections = listSections(tasks, display, groupContext)
  const projectById = new Map(projects.map((project) => [project.id, project]))
  const statusOptions = groupTasks([], 'status', { ...groupContext, showEmpty: true })
    .filter((group) => !group.value?.startsWith('duplicate:'))

  const toggleSelect = (taskId: string) =>
    setSelected((prev) => (prev.includes(taskId) ? prev.filter((id) => id !== taskId) : [...prev, taskId]))

  const selectedTasks = tasks.filter((t) => selected.includes(t.id))

  if (tasks.length === 0) {
    return (
      <EmptyState
        icon={SquareCheck}
        title="No tasks found"
        description="No tasks match the current filters. Try clearing a filter or create a new task."
      />
    )
  }

  const renderRows = (zone: string, values: GroupValues, zoneTasks: Task[]) => {
    // the dragged row stays mounted (faded): unmounting the drag source cancels the browser drag
    const others = drag ? zoneTasks.filter((task) => task.id !== drag.taskId) : zoneTasks
    const index = drop?.zone === zone ? drop.index : null
    return zoneTasks.map((task) => {
      const slot = others.indexOf(task)
      const dropEdge = index === null || slot === -1
        ? null
        : slot === index ? 'top' : index === others.length && slot === others.length - 1 ? 'bottom' : null
      return (
        <TaskRow
          key={task.id}
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
          dropEdge={dropEdge}
          onOpen={onOpen}
          onToggleSelect={toggleSelect}
          onDragStart={(taskId) => startDrag(taskId, values)}
          onDragEnd={endDrag}
          onRequestDuplicate={(rowTask) => setDuplicatePicker([rowTask])}
        />
      )
    })
  }

  const renderHeader = (group: TaskGroup, zone: string, values: GroupValues, level: 'group' | 'sub') => {
    const isCollapsed = collapsed.includes(zone)
    // inside the Duplicate status (group or sub-group) a new task would need a canonical task first
    const canAdd = !values.some((value) => value.field === 'status' && value.value?.startsWith('duplicate:'))
    return (
      <div className={level === 'group' ? GROUP_HEADER : SUB_HEADER}>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className={TOGGLE}
          aria-expanded={!isCollapsed}
          aria-label={`${isCollapsed ? 'Expand' : 'Collapse'} ${group.label}`}
          onClick={() => toggle(zone)}
        >
          <ChevronRight className={cn(CHEVRON, !isCollapsed && 'rotate-90')} />
        </Button>
        <GroupIcon group={group} context={groupContext} />
        <span className="truncate">{group.label}</span>
        <span className="font-normal text-muted-foreground/70 tabular-nums">{group.tasks.length}</span>
        <div className="flex-1" />
        {canAdd ? (
          <Button type="button" variant="ghost" size="icon-xs" className={ADD} aria-label={`New task in ${group.label}`} title="New task" onClick={() => onAdd(values)}>
            <Plus className="size-3.5" />
          </Button>
        ) : null}
      </div>
    )
  }

  return (
    <>
      {sections.map(({ group, subGroups }) => {
        const values = valuesOf(group)
        const zone = zoneIdOf(values)
        if (group.field === 'none') {
          return <section key={zone} className={ZONE} {...zoneProps({ id: zone, values, tasks: group.tasks, rowsShown: true })}>{renderRows(zone, values, group.tasks)}</section>
        }
        const isCollapsed = collapsed.includes(zone)
        if (!subGroups) {
          return (
            <section key={group.key} className={cn('group/section', ZONE)} {...zoneProps({ id: zone, values, tasks: group.tasks, rowsShown: !isCollapsed })}>
              {renderHeader(group, zone, values, 'group')}
              {isCollapsed ? null : renderRows(zone, values, group.tasks)}
            </section>
          )
        }
        if (isCollapsed) {
          // the sub-groups are hidden, so the collapsed group itself takes drops: only the group field changes
          // (a hidden sub-group value must never be written) and the task goes to the group's end
          return (
            <section key={group.key} className={cn('group/section', ZONE)} {...zoneProps({ id: zone, values, tasks: group.tasks, rowsShown: false })}>
              {renderHeader(group, zone, values, 'group')}
            </section>
          )
        }
        return (
          <section key={group.key} className="group/section">
            {renderHeader(group, zone, values, 'group')}
            {subGroups.map((sub) => {
              const subValues = valuesOf(group, sub)
              const subZone = zoneIdOf(subValues)
              const subCollapsed = collapsed.includes(subZone)
              return (
                <section key={sub.key} className={cn('group/sub', ZONE)} {...zoneProps({ id: subZone, values: subValues, tasks: sub.tasks, rowsShown: !subCollapsed })}>
                  {renderHeader(sub, subZone, subValues, 'sub')}
                  {subCollapsed ? null : renderRows(subZone, subValues, sub.tasks)}
                </section>
              )
            })}
          </section>
        )
      })}
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
    </>
  )
}

/** Floating toolbar for the selected rows: bulk status, priority, assignees and labels. */
function BulkBar({
  tasks,
  users,
  statuses,
  statusOptions,
  groupContext,
  labels,
  onClear,
  onMarkDuplicate,
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
}) {
  const { workspace } = useWorkspace()
  const bulkTasks = useBulkTasks(workspace.id)
  const limitError = bulkTasks.error instanceof BulkTaskLimitError ? bulkTasks.error : null

  // Esc clears the selection, unless it belongs to a field, an open menu or a dialog
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]')) return
      // closed Base UI popups stay mounted with data-closed until their exit animation ends
      if (document.querySelector(['menu', 'dialog', 'alertdialog', 'listbox'].map((role) => `[role="${role}"]:not([data-closed])`).join())) return
      onClear()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClear])

  const bulkStatus = (key: string | null) => {
    const updates = tasks.flatMap((task) => {
      const statusId = resolveStatusId(statuses, task.projectId, key)
      return statusId && statusId !== task.statusId
        ? [{ id: task.id, expected_version: task.version, status_id: statusId }]
        : []
    })
    if (updates.length > 0) bulkTasks.mutate(updates)
  }
  const bulkPriority = (priority: Task['priority']) => {
    const updates = tasks.filter((task) => task.priority !== priority).map((task) => ({ id: task.id, expected_version: task.version, priority }))
    if (updates.length > 0) bulkTasks.mutate(updates)
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
  const bulkLabel = (labelId: string) => {
    const everyone = tasks.every((t) => t.labels.includes(labelId))
    bulkTasks.mutate(tasks.map((task) => ({
      id: task.id,
      expected_version: task.version,
      label_ids: everyone ? task.labels.filter((item) => item !== labelId) : Array.from(new Set([...task.labels, labelId])),
    })))
  }

  return (
    // outer layer centres the bar, so the enter animation can own the bar's transform
    <div className="pointer-events-none absolute inset-x-0 bottom-4 z-40 flex justify-center px-6 max-[899px]:bottom-2.5 max-[899px]:px-2">
      <div
        className="pointer-events-auto flex max-w-full items-center gap-0.5 overflow-x-auto rounded-xl border border-border bg-popover p-1 text-popover-foreground shadow-[0_12px_32px_-8px_rgb(0_0_0/0.35),0_2px_6px_-2px_rgb(0_0_0/0.18)] duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] animate-in fade-in-0 slide-in-from-bottom-2 zoom-in-97 max-[899px]:w-full [scrollbar-width:none] dark:ring-1 dark:ring-white/5"
        role="toolbar"
        aria-label="Selected tasks"
      >
        <div className="flex h-8 shrink-0 items-center rounded-lg bg-primary/12 pl-3 text-primary dark:bg-primary/25 dark:text-primary-foreground">
          <span className="text-xs font-semibold whitespace-nowrap tabular-nums">{tasks.length} selected</span>
          <Button
            variant="ghost"
            size="icon-xs"
            className="mx-1 text-current opacity-70 hover:bg-primary/15 hover:text-current hover:opacity-100 dark:hover:bg-white/10"
            aria-label="Clear selection"
            title="Clear selection (Esc)"
            onClick={onClear}
          >
            <X className="size-3.5" />
          </Button>
        </div>
        <div className="mx-1 h-4 w-px shrink-0 bg-border" aria-hidden />
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="ghost" className={BULK_BTN}><RecordCircle aria-hidden className={BULK_ICON} /><span className={BULK_LABEL}>Status</span></Button>} />
          <DropdownMenuContent side="top" className={MENU}>
            {statusOptions.map((group) => (
              <DropdownMenuItem key={group.key} className={OPTION} onClick={() => bulkStatus(group.value)}>
                <GroupIcon group={group} context={groupContext} />
                {group.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="ghost" className={BULK_BTN}><Flag aria-hidden className={BULK_ICON} /><span className={BULK_LABEL}>Priority</span></Button>} />
          <DropdownMenuContent side="top" className={MENU}>
            {PRIORITY_ORDER.map((priority) => (
              <DropdownMenuItem key={priority} className={OPTION} onClick={() => bulkPriority(priority)}>
                <PriorityIcon priority={priority} />
                {PRIORITY_LABEL[priority]}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="ghost" className={BULK_BTN}><UserAdd aria-hidden className={BULK_ICON} /><span className={BULK_LABEL}>Assignee</span></Button>} />
          <DropdownMenuContent side="top" align="end" className={MENU}>
            {users.map((u) => {
              const everyone = tasks.every((t) => t.assigneeIds.includes(u.id))
              return (
                <DropdownMenuCheckboxItem key={u.id} className={CHECK_OPTION} checked={everyone} closeOnClick onCheckedChange={() => bulkAssign(u.id)}>
                  <UserAvatar user={u} size={16} />
                  {u.name}
                </DropdownMenuCheckboxItem>
              )
            })}
          </DropdownMenuContent>
        </DropdownMenu>
        {/* label toggles keep the menu open (closeOnClick defaults to false on checkbox items) so several can be flipped in a row */}
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="ghost" className={BULK_BTN}><Tag aria-hidden className={BULK_ICON} /><span className={BULK_LABEL}>Labels</span></Button>} />
          <DropdownMenuContent side="top" align="end" className={MENU}>
            {labels.map((label) => {
              const everyone = tasks.every((t) => t.labels.includes(label.id))
              return (
                <DropdownMenuCheckboxItem key={label.id} className={CHECK_OPTION} checked={everyone} onCheckedChange={() => bulkLabel(label.id)}>
                  <Badge variant="outline" className={PILL}><span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />{label.name}</Badge>
                </DropdownMenuCheckboxItem>
              )
            })}
          </DropdownMenuContent>
        </DropdownMenu>
        <Button variant="ghost" className={BULK_BTN} aria-label="Mark as duplicate…" title="Mark as duplicate…" onClick={onMarkDuplicate}>
          <Copy aria-hidden className={BULK_ICON} />
          <span className={BULK_LABEL}>Duplicate</span>
        </Button>
        {bulkTasks.isPending ? (
          <span role="status" className="flex shrink-0 items-center gap-1.5 px-2 text-xs whitespace-nowrap text-muted-foreground">
            <Loader aria-hidden className="size-3.5 animate-spin" />
            Updating…
          </span>
        ) : null}
        {bulkTasks.isError ? (
          <span role="alert" className="flex min-w-0 items-center gap-1.5 rounded-lg bg-destructive/10 py-1 pr-1 pl-2 text-xs text-destructive">
            <Danger aria-hidden className="size-3.5 shrink-0" />
            {limitError ? `This update includes ${limitError.count} tasks. Select ${MAX_BULK_TASK_UPDATES} or fewer and try again.` : <>Bulk update failed. <Button variant="ghost" size="xs" className="text-destructive hover:bg-destructive/15 hover:text-destructive" onClick={bulkTasks.retry}>Retry</Button></>}
          </span>
        ) : null}
      </div>
    </div>
  )
}
