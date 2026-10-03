import { useRef, useState, type ReactNode } from 'react'
import { toast } from 'sonner'
import { ArrowUpRightSquare, Calendar, Clipboard, Copy, Flag, Folder, Hierarchy2, LinkBroken, RecordCircle, Tag, Trash, User as UserIcon, UserAdd } from 'reicon-react'
import type { BulkItem, LabelRecord } from '@/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { UserAvatar } from '@/components/common/UserAvatar'
import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useProjects } from '@/features/tasks/api/projects'
import { useBulkTasks } from '@/features/tasks/api/tasks'
import { assignUpdates, assigneeToggleUpdates, dueUpdates, labelToggleUpdates, priorityUpdates, statusUpdates } from '@/features/tasks/bulkUpdates'
import { pickerTitle } from '@/features/tasks/relationsLib'
import { parentPickerTitle } from '@/features/tasks/subIssuesLib'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import { dueDatePresets, menuTargetIds } from '@/features/tasks/taskMenuLib'
import { useDuplicateActions } from '@/features/tasks/useDuplicateActions'
import { useMoveToProject } from '@/features/tasks/useMoveToProject'
import { useParentActions } from '@/features/tasks/useParentActions'
import { useTrashTasks } from '@/features/tasks/useTrashTasks'
import { GroupIcon } from '@/features/views/components/GroupIcon'
import { groupTasks, type GroupContext } from '@/features/views/grouping'
import { buildTaskTree, descendantIds } from '@/features/views/taskTree'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { taskPath } from '@/lib/taskLinks'
import { Shortcut } from '@/shortcuts/Shortcut'
import { useTaskTarget } from '@/shortcuts/taskTarget'
import { DueDateDialog } from './DueDateDialog'
import { PriorityIcon } from './PriorityIcon'
import { LabelPill } from './TaskLabels'
import { TaskPickerDialog } from './TaskPickerDialog'

interface TaskContextMenuProps {
  /** The tasks in view. */
  tasks: Task[]
  users: User[]
  labels: LabelRecord[]
  statuses: TaskStatusDef[]
  groupContext: GroupContext
  currentUserId: string
  /** The list, the board or the timeline: every element with a task id in it opens the menu. */
  children: ReactNode
}

const DAY_FORMAT = new Intl.DateTimeFormat(undefined, { weekday: 'short', day: 'numeric', month: 'short' })

/** The right-click menu of task rows and board cards. It acts on the selection when the row is part of it, else on
 *  the row alone. One menu for the whole list: a menu for each row would mount hundreds of them. */
export function TaskContextMenu({ tasks, users, labels, statuses, groupContext, currentUserId, children }: TaskContextMenuProps) {
  const { workspace } = useWorkspace()
  const { selectedIds, setSelected } = useTaskTarget()
  const bulkTasks = useBulkTasks(workspace.id)
  const trashTasks = useTrashTasks(workspace.id)
  const duplicates = useDuplicateActions(workspace.id)
  const parentActions = useParentActions(workspace.id)
  const moveToProject = useMoveToProject(workspace.id)
  // every project: a project page's group context knows only its own
  const projects = useProjects(workspace.id).data ?? []
  const pickedId = useRef<string | null>(null)
  const [ids, setIds] = useState<string[]>([])
  // a dialog that follows the menu; it keeps the tasks the menu was opened on
  const [dialog, setDialog] = useState<'dueDate' | 'duplicate' | 'parent' | null>(null)

  /** Remembers the task under the pointer; false when the pointer is not on a task or is on a link (the browser menu shows). */
  const pick = (target: EventTarget) => {
    const element = target instanceof Element ? target : null
    // a timeline row is not a keyboard target (its bar is), so it names its task with data-task-menu
    const row = element?.closest('[data-task-id], [data-task-menu]')
    const rowId = row?.getAttribute('data-task-id') ?? row?.getAttribute('data-task-menu')
    if (!rowId || element?.closest('a[href]')) return false
    pickedId.current = rowId
    return true
  }
  /** The menu opens: only now the row takes effect, because a touch that scrolls the list also starts on a row. */
  const onOpenChange = (open: boolean) => {
    const rowId = pickedId.current
    if (!open || !rowId) return
    setIds(menuTargetIds(rowId, selectedIds))
    // a row outside the selection takes over from it
    if (selectedIds.length > 0 && !selectedIds.includes(rowId)) setSelected([])
  }

  // the live tasks, so that a second choice in an open submenu sends the current versions
  const targets = tasks.filter((task) => ids.includes(task.id))
  const [first] = targets
  const scope = targets.length === 1 ? first.identifier : targets.length
  const mutate = (updates: BulkItem[]) => {
    if (updates.length > 0) bulkTasks.mutate(updates)
  }
  const setDue = (start: string | null, end: string | null) => mutate(dueUpdates(targets, start, end))
  const copy = (text: string, done: string) => void navigator.clipboard.writeText(text).then(() => toast(done), () => toast.error('Could not copy to the clipboard.'))
  const statusOptions = groupTasks([], 'status', { ...groupContext, showEmpty: true }).filter((group) => !group.value?.startsWith('duplicate:'))
  const closeDialog = () => setDialog(null)

  return (
    <ContextMenu onOpenChange={onOpenChange}>
      <ContextMenuTrigger
        // the trigger adds no box and keeps text selection as it was
        className="contents select-auto"
        onContextMenu={(event) => {
          if (pick(event.target)) return
          event.preventBaseUIHandler()
          // Base UI also cancels the browser menu from a document listener
          event.nativeEvent.stopPropagation()
        }}
        onTouchStart={(event) => {
          if (!pick(event.target)) event.preventBaseUIHandler()
        }}
      >
        {children}
      </ContextMenuTrigger>
      {targets.length > 0 ? (
        <ContextMenuContent className="min-w-56">
          {targets.length > 1 ? (
            <ContextMenuGroup>
              <ContextMenuLabel>{targets.length} tasks</ContextMenuLabel>
            </ContextMenuGroup>
          ) : null}
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <RecordCircle aria-hidden />
              <span className="flex-1">Status</span>
              <ContextMenuShortcut><Shortcut id="task.setStatus" /></ContextMenuShortcut>
            </ContextMenuSubTrigger>
            <ContextMenuSubContent className="min-w-45">
              {statusOptions.map((group) => {
                const updates = statusUpdates(targets, statuses, group.value)
                return (
                  <ContextMenuCheckboxItem key={group.key} checked={updates.length === 0} closeOnClick onCheckedChange={() => mutate(updates)}>
                    <GroupIcon group={group} context={groupContext} />
                    {group.label}
                  </ContextMenuCheckboxItem>
                )
              })}
            </ContextMenuSubContent>
          </ContextMenuSub>
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <Flag aria-hidden />
              <span className="flex-1">Priority</span>
              <ContextMenuShortcut><Shortcut id="task.setPriority" /></ContextMenuShortcut>
            </ContextMenuSubTrigger>
            <ContextMenuSubContent className="min-w-45">
              {PRIORITY_ORDER.map((priority) => (
                <ContextMenuCheckboxItem key={priority} checked={targets.every((task) => task.priority === priority)} closeOnClick onCheckedChange={() => mutate(priorityUpdates(targets, priority))}>
                  <PriorityIcon priority={priority} />
                  {PRIORITY_LABEL[priority]}
                </ContextMenuCheckboxItem>
              ))}
            </ContextMenuSubContent>
          </ContextMenuSub>
          {/* assignees and labels are sets: the submenu stays open so several can be flipped in a row */}
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <UserAdd aria-hidden />
              <span className="flex-1">Assignee</span>
              <ContextMenuShortcut><Shortcut id="task.setAssignee" /></ContextMenuShortcut>
            </ContextMenuSubTrigger>
            <ContextMenuSubContent className="min-w-45">
              {users.map((user) => (
                <ContextMenuCheckboxItem key={user.id} checked={targets.every((task) => task.assigneeIds.includes(user.id))} onCheckedChange={() => mutate(assigneeToggleUpdates(targets, user.id))}>
                  <UserAvatar user={user} size={16} />
                  {user.name}
                </ContextMenuCheckboxItem>
              ))}
            </ContextMenuSubContent>
          </ContextMenuSub>
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <Calendar aria-hidden />
              <span className="flex-1">Due date</span>
              <ContextMenuShortcut><Shortcut id="task.setDueDate" /></ContextMenuShortcut>
            </ContextMenuSubTrigger>
            <ContextMenuSubContent className="min-w-52">
              {dueDatePresets(new Date()).map((preset) => (
                <ContextMenuItem key={preset.label} onClick={() => setDue(null, preset.date.toISOString())}>
                  {preset.label}
                  <ContextMenuShortcut className="tracking-normal">{DAY_FORMAT.format(preset.date)}</ContextMenuShortcut>
                </ContextMenuItem>
              ))}
              <ContextMenuSeparator />
              <ContextMenuItem onClick={() => setDialog('dueDate')}>Custom…</ContextMenuItem>
              {targets.some((task) => task.dueAt) ? <ContextMenuItem onClick={() => setDue(null, null)}>Remove due date</ContextMenuItem> : null}
            </ContextMenuSubContent>
          </ContextMenuSub>
          {labels.length > 0 ? (
            <ContextMenuSub>
              <ContextMenuSubTrigger>
                <Tag aria-hidden />
                <span className="flex-1">Labels</span>
                <ContextMenuShortcut><Shortcut id="task.setLabels" /></ContextMenuShortcut>
              </ContextMenuSubTrigger>
              <ContextMenuSubContent className="min-w-45">
                {labels.map((label) => (
                  <ContextMenuCheckboxItem key={label.id} checked={targets.every((task) => task.labels.includes(label.id))} onCheckedChange={() => mutate(labelToggleUpdates(targets, label.id))}>
                    <LabelPill label={label} />
                  </ContextMenuCheckboxItem>
                ))}
              </ContextMenuSubContent>
            </ContextMenuSub>
          ) : null}
          {projects.length > 1 ? (
            <ContextMenuSub>
              <ContextMenuSubTrigger>
                <Folder aria-hidden />
                <span className="flex-1">Move to project</span>
                <ContextMenuShortcut><Shortcut id="task.moveToProject" /></ContextMenuShortcut>
              </ContextMenuSubTrigger>
              <ContextMenuSubContent className="min-w-52">
                {projects.map((project) => (
                  <ContextMenuCheckboxItem key={project.id} checked={targets.every((task) => task.projectId === project.id)} closeOnClick onCheckedChange={() => moveToProject(targets, project)}>
                    <ColorDot color={project.color} className="size-2" />
                    <span className="flex-1 truncate">{project.name}</span>
                    <span className="text-xs text-muted-foreground tabular-nums">{project.key}</span>
                  </ContextMenuCheckboxItem>
                ))}
              </ContextMenuSubContent>
            </ContextMenuSub>
          ) : null}
          <ContextMenuSeparator />
          {targets.some((task) => !task.assigneeIds.includes(currentUserId)) ? (
            <ContextMenuItem onClick={() => mutate(assignUpdates(targets, currentUserId))}>
              <UserIcon aria-hidden />
              Assign to me
              <ContextMenuShortcut><Shortcut id="task.assignMe" /></ContextMenuShortcut>
            </ContextMenuItem>
          ) : null}
          <ContextMenuItem onClick={() => setDialog('parent')}>
            <Hierarchy2 aria-hidden />
            Set parent…
          </ContextMenuItem>
          {targets.some((task) => task.parentTaskId) ? (
            <ContextMenuItem onClick={() => void parentActions.setParent(targets, null)}>
              <LinkBroken aria-hidden />
              Remove parent
            </ContextMenuItem>
          ) : null}
          <ContextMenuItem onClick={() => setDialog('duplicate')}>
            <Copy aria-hidden />
            Mark as duplicate…
          </ContextMenuItem>
          <ContextMenuSeparator />
          {/* one task only: these have no form for several tasks */}
          {targets.length === 1 ? (
            <>
              <ContextMenuSub>
                <ContextMenuSubTrigger>
                  <Clipboard aria-hidden />
                  <span className="flex-1">Copy</span>
                </ContextMenuSubTrigger>
                <ContextMenuSubContent className="min-w-45">
                  <ContextMenuItem onClick={() => copy(first.identifier, 'Copied task ID')}>
                    ID
                    <ContextMenuShortcut><Shortcut id="task.copyId" /></ContextMenuShortcut>
                  </ContextMenuItem>
                  <ContextMenuItem onClick={() => copy(`${window.location.origin}${taskPath(first)}`, 'Copied task link')}>
                    Link
                    <ContextMenuShortcut><Shortcut id="task.copyLink" /></ContextMenuShortcut>
                  </ContextMenuItem>
                  <ContextMenuItem onClick={() => copy(first.title, 'Copied task title')}>Title</ContextMenuItem>
                </ContextMenuSubContent>
              </ContextMenuSub>
              <ContextMenuItem onClick={() => window.open(taskPath(first), '_blank', 'noopener')}>
                <ArrowUpRightSquare aria-hidden />
                Open in new tab
              </ContextMenuItem>
              <ContextMenuSeparator />
            </>
          ) : null}
          <ContextMenuItem
            onClick={async () => {
              if (await trashTasks(targets)) setSelected([])
            }}
          >
            <Trash aria-hidden />
            Move to trash
            <ContextMenuShortcut><Shortcut id="task.trash" /></ContextMenuShortcut>
          </ContextMenuItem>
        </ContextMenuContent>
      ) : null}
      {dialog === 'dueDate' && targets.length > 0 ? (
        <DueDateDialog
          tasks={targets}
          onClose={closeDialog}
          onPick={(start, end) => {
            setDue(start, end)
            closeDialog()
          }}
        />
      ) : null}
      {dialog === 'duplicate' && targets.length > 0 ? (
        <TaskPickerDialog
          open
          onOpenChange={(open) => { if (!open) closeDialog() }}
          title={pickerTitle('duplicate', scope)}
          statuses={statuses}
          excludeIds={ids}
          excludeDuplicates
          onSelect={(target) => {
            if (targets.length === 1) void duplicates.markOne(first, target)
            else void duplicates.markMany(targets, target)
            setSelected([])
          }}
        />
      ) : null}
      {dialog === 'parent' && targets.length > 0 ? (
        <TaskPickerDialog
          open
          onOpenChange={(open) => { if (!open) closeDialog() }}
          title={parentPickerTitle(scope)}
          statuses={statuses}
          // loaded descendants only; a deeper one is refused by the server (parent_cycle) and explained in a toast
          excludeIds={targets.flatMap((task) => [task.id, ...descendantIds(buildTaskTree(tasks), task.id)])}
          onSelect={(target) => {
            void parentActions.setParent(targets, { id: target.id, identifier: target.identifier })
            setSelected([])
          }}
        />
      ) : null}
    </ContextMenu>
  )
}
