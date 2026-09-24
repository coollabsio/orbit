import { useEffect, useState } from 'react'
import { ChevronRight, Copy, Danger, Flag, Loader, Add as Plus, RecordCircle, TaskSquare as SquareCheck, Tag, UserAdd, Xmark as X } from 'reicon-react'
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
import { TaskStatusIcon } from './TaskStatusIcon'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { LabelRecord } from '@/api/generated/types.gen'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES, useBulkTasks, useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { groupTasksByStatus, resolveStatusId, type SortKey, type StatusGroup } from '@/features/tasks/tasksLib'
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

interface TaskListProps {
  tasks: Task[]
  users: User[]
  labels: LabelRecord[]
  statuses: TaskStatusDef[]
  groups: StatusGroup[]
  sort: SortKey
  onOpen: (taskId: string) => void
  onAdd: (statusKey: string) => void
}

const collapsedStorageKey = (workspaceId: string) => `orbit:task_list_collapsed:${workspaceId}`

function storedCollapsedGroups(workspaceId: string): string[] {
  try {
    const value = JSON.parse(window.localStorage.getItem(collapsedStorageKey(workspaceId)) ?? '[]')
    return Array.isArray(value) ? value.filter((key): key is string => typeof key === 'string') : []
  } catch {
    return []
  }
}

/** Status groups: collapsible headers that also accept dropped rows (moves the task to that status). */
export function TaskList({ tasks, users, labels, statuses, groups, sort, onOpen, onAdd }: TaskListProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const taskGroups = groupTasksByStatus(tasks, groups, sort)
  const [collapsed, setCollapsed] = useState<string[]>(() => storedCollapsedGroups(workspace.id))
  const [selected, setSelected] = useState<string[]>([])
  const [draggingId, setDraggingId] = useState<string | null>(null)
  const [dropKey, setDropKey] = useState<string | null>(null)
  const duplicates = useDuplicateActions(workspace.id)
  // tasks waiting for a canonical task: one (row menu, drop on the Duplicate group) or the bulk selection
  const [duplicatePicker, setDuplicatePicker] = useState<Task[] | null>(null)

  const toggle = (key: string) => setCollapsed((prev) => {
    const next = prev.includes(key) ? prev.filter((item) => item !== key) : [...prev, key]
    window.localStorage.setItem(collapsedStorageKey(workspace.id), JSON.stringify(next))
    return next
  })
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
        icon={SquareCheck}
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
            className="group/section data-[drop-over]:ring-1 data-[drop-over]:ring-primary/30 data-[drop-over]:ring-inset"
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
                  if (statuses.find((status) => status.id === statusId)?.category === 'duplicate') setDuplicatePicker([task])
                  else updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: statusId } })
                }
              }
              endDrag()
            }}
          >
            <div className="group/hdr sticky top-0 z-[5] flex h-9 items-center gap-2 border-b border-border bg-card pr-2 pl-1.5 text-xs font-semibold text-muted-foreground transition-colors group-data-[drop-over]/section:bg-primary/10 group-data-[drop-over]/section:text-primary">
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                className="size-5 rounded-md border-0 text-muted-foreground/70 transition-colors hover:bg-accent hover:text-foreground dark:hover:bg-accent"
                aria-expanded={!isCollapsed}
                aria-label={isCollapsed ? `Expand ${group.name}` : `Collapse ${group.name}`}
                onClick={() => toggle(group.key)}
              >
                <ChevronRight className={cn('size-3 transition-transform', !isCollapsed && 'rotate-90')} />
              </Button>
              <TaskStatusIcon status={group.status} />
              <span>{group.name}</span>
              <span className="font-normal text-muted-foreground/70 tabular-nums">{group.tasks.length}</span>
              <div className="flex-1" />
              {group.category === 'duplicate' ? null : (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  className="size-6 rounded-md border-0 text-muted-foreground/70 opacity-0 transition hover:bg-accent hover:text-foreground group-hover/hdr:opacity-100 focus-visible:opacity-100 dark:hover:bg-accent"
                  aria-label={`New task in ${group.name}`}
                  title="New task"
                  onClick={() => onAdd(group.key)}
                >
                  <Plus className="size-3.5" />
                </Button>
              )}
            </div>
            {!isCollapsed
              ? group.tasks.map((task) => (
                  <TaskRow
                    key={task.id}
                    task={task}
                    labels={labels}
                    statuses={statuses}
                    users={users}
                    assignees={users.filter((u) => task.assigneeIds.includes(u.id))}
                    selected={selected.includes(task.id)}
                    dragging={task.id === draggingId}
                    onOpen={onOpen}
                    onToggleSelect={toggleSelect}
                    onDragStart={setDraggingId}
                    onDragEnd={endDrag}
                    onRequestDuplicate={(rowTask) => setDuplicatePicker([rowTask])}
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
  groups,
  labels,
  onClear,
  onMarkDuplicate,
}: {
  tasks: Task[]
  users: User[]
  statuses: TaskStatusDef[]
  groups: StatusGroup[]
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

  const bulkStatus = (key: string) => {
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
            {groups.filter((group) => group.category !== 'duplicate').map((group) => (
              <DropdownMenuItem key={group.key} className={OPTION} onClick={() => bulkStatus(group.key)}>
                <TaskStatusIcon status={group.status} />
                {group.name}
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
        <Button variant="ghost" className={BULK_BTN} onClick={onMarkDuplicate}>
          <Copy aria-hidden className={BULK_ICON} />
          <span className={BULK_LABEL}>Mark as duplicate…</span>
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
