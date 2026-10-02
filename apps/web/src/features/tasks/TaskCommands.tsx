import { useState } from 'react'
import { toast } from 'sonner'
import type { BulkItem, LabelRecord } from '@/api/generated/types.gen'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Command, CommandDialog, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useBulkTasks } from '@/features/tasks/api/tasks'
import { assignUpdates, assigneeToggleUpdates, dueUpdates, labelToggleUpdates, priorityUpdates, statusUpdates } from '@/features/tasks/bulkUpdates'
import { DueDateDialog } from '@/features/tasks/components/DueDateDialog'
import { PriorityIcon } from '@/features/tasks/components/PriorityIcon'
import { LabelPill } from '@/features/tasks/components/TaskLabels'
import { PRIORITY_LABEL, PRIORITY_ORDER } from '@/features/tasks/taskMeta'
import { useTrashTasks } from '@/features/tasks/useTrashTasks'
import { GroupIcon } from '@/features/views/components/GroupIcon'
import { groupTasks, type GroupContext } from '@/features/views/grouping'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { focusTaskRow, focusedTaskId, useTaskTarget } from '@/shortcuts/taskTarget'
import { useCommand } from '@/shortcuts/useCommand'

type TaskField = 'status' | 'priority' | 'assignee' | 'labels' | 'dueDate'

const FIELD_TITLE: Record<TaskField, string> = { status: 'Change status', priority: 'Change priority', assignee: 'Change assignee', labels: 'Change labels', dueDate: 'Set due date' }

interface TaskCommandsProps {
  /** The tasks in view, or the open task. */
  tasks: Task[]
  users: User[]
  labels: LabelRecord[]
  statuses: TaskStatusDef[]
  groupContext: GroupContext
  currentUserId: string
}

/** The task commands of the keyboard and the command menu. They act on the target tasks: the selection, the focused
 *  row, the row under the pointer, or the open task. A property command opens one list of options for all of them. */
export function TaskCommands({ tasks, users, labels, statuses, groupContext, currentUserId }: TaskCommandsProps) {
  const { workspace } = useWorkspace()
  const { getTargetIds, openTaskId, setSelected } = useTaskTarget()
  const bulkTasks = useBulkTasks(workspace.id)
  const trashTasks = useTrashTasks(workspace.id)
  // `focused`: the row that had the keyboard focus, to give it back when the list closes
  const [menu, setMenu] = useState<{ field: TaskField; ids: string[]; focused: string | null } | null>(null)

  const targets = () => {
    const ids = getTargetIds()
    return tasks.filter((task) => ids.includes(task.id))
  }
  const available = () => targets().length > 0
  const mutate = (updates: BulkItem[]) => {
    if (updates.length > 0) bulkTasks.mutate(updates)
  }
  const open = (field: TaskField) => () => setMenu({ field, ids: targets().map((task) => task.id), focused: focusedTaskId() })
  // one task only: these have no form for several tasks
  const single = () => targets().length === 1
  const copy = (text: string, done: string) => void navigator.clipboard.writeText(text).then(() => toast(done), () => toast.error('Could not copy to the clipboard.'))

  useCommand('task.setStatus', open('status'), { available })
  useCommand('task.setPriority', open('priority'), { available })
  useCommand('task.setAssignee', open('assignee'), { available })
  useCommand('task.setLabels', open('labels'), { available })
  useCommand('task.setDueDate', open('dueDate'), { available })
  useCommand('task.assignMe', () => mutate(assignUpdates(targets(), currentUserId)), { available })
  useCommand('task.copyId', () => copy(targets()[0].identifier, 'Copied task ID'), { available: single })
  useCommand('task.copyLink', () => copy(`${window.location.origin}/tasks/${targets()[0].id}`, 'Copied task link'), { available: single })
  // the open task has its own trash action, which also closes the page
  useCommand('task.trash', openTaskId ? null : async () => {
    if (await trashTasks(targets())) setSelected([])
  }, { available })

  // the live tasks, so that a second choice in the same list sends the current versions
  const menuTasks = menu ? tasks.filter((task) => menu.ids.includes(task.id)) : []
  if (!menu || menuTasks.length === 0) return null
  const close = () => {
    setMenu(null)
    // a change can move the row to another group, which mounts it again: the keyboard stays on the task
    const row = menu.focused
    if (row) setTimeout(() => {
      if (focusedTaskId() !== row) focusTaskRow(row)
    }, 50)
  }
  const scope = menuTasks.length === 1 ? menuTasks[0].identifier : `${menuTasks.length} tasks`

  if (menu.field === 'dueDate') {
    return (
      <DueDateDialog
        tasks={menuTasks}
        onClose={close}
        onPick={(start, end) => {
          mutate(dueUpdates(menuTasks, start, end))
          close()
        }}
      />
    )
  }

  const statusOptions = groupTasks([], 'status', { ...groupContext, showEmpty: true }).filter((group) => !group.value?.startsWith('duplicate:'))
  const statusKeys = new Set(statusOptions.filter((group) => statusUpdates(menuTasks, statuses, group.value).length === 0).map((group) => group.key))
  /** One choice is the answer: send it and close. */
  const choose = (updates: BulkItem[]) => {
    mutate(updates)
    close()
  }

  return (
    <CommandDialog open onOpenChange={(next) => { if (!next) close() }} title={FIELD_TITLE[menu.field]} description={scope} className="flex max-h-[60vh] flex-col sm:max-w-md">
      <Command className="min-h-0 bg-transparent p-0">
        <div className="px-3 pt-2.5 pb-1">
          <span className="rounded-sm bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">{scope}</span>
        </div>
        <CommandInput autoFocus placeholder={`${FIELD_TITLE[menu.field]}…`} />
        <CommandList className="mx-1.5 mt-1 mb-1.5 max-h-none min-h-0 flex-1 rounded-lg bg-background p-1 ring-1 ring-border">
          <CommandEmpty className="p-6 text-[13px] text-muted-foreground">No match</CommandEmpty>
          {menu.field === 'status' ? statusOptions.map((group) => (
            <CommandItem key={group.key} value={group.label} data-checked={statusKeys.has(group.key)} onSelect={() => choose(statusUpdates(menuTasks, statuses, group.value))}>
              <GroupIcon group={group} context={groupContext} />
              {group.label}
            </CommandItem>
          )) : null}
          {menu.field === 'priority' ? PRIORITY_ORDER.map((priority) => (
            <CommandItem key={priority} value={PRIORITY_LABEL[priority]} data-checked={menuTasks.every((task) => task.priority === priority)} onSelect={() => choose(priorityUpdates(menuTasks, priority))}>
              <PriorityIcon priority={priority} />
              {PRIORITY_LABEL[priority]}
            </CommandItem>
          )) : null}
          {/* assignees and labels are sets: the list stays open so several can be flipped in a row */}
          {menu.field === 'assignee' ? users.map((user) => (
            <CommandItem key={user.id} value={user.name} data-checked={menuTasks.every((task) => task.assigneeIds.includes(user.id))} onSelect={() => mutate(assigneeToggleUpdates(menuTasks, user.id))}>
              <UserAvatar user={user} size={16} />
              {user.name}
            </CommandItem>
          )) : null}
          {menu.field === 'labels' ? labels.map((label) => (
            <CommandItem key={label.id} value={label.name} data-checked={menuTasks.every((task) => task.labels.includes(label.id))} onSelect={() => mutate(labelToggleUpdates(menuTasks, label.id))}>
              <LabelPill label={label} />
            </CommandItem>
          )) : null}
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
