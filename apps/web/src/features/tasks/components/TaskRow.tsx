import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { TaskStatusIcon } from './TaskStatusIcon'
import { projectStatuses } from '@/features/tasks/taskMeta'
import { shortDate } from '@/lib/format'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { LabelRecord } from '@/api/generated/types.gen'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { PriorityPicker } from './PriorityPicker'
import { LinkifiedText } from './LinkifiedText'

const PILL = 'inline-flex h-[22px] items-center gap-1.5 overflow-visible rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`
/** Multi-select rows keep room on the right for the checked indicator. */
const CHECK_OPTION =
  `group min-h-8 cursor-pointer gap-2 py-1.5 pr-8 pl-2 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-checked:bg-accent data-checked:font-medium`
const HEADING = 'px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase'

interface TaskRowProps {
  task: Task
  statuses: TaskStatusDef[]
  labels: LabelRecord[]
  users: User[]
  assignees: User[]
  selected: boolean
  dragging: boolean
  onOpen: (taskId: string) => void
  onToggleSelect: (taskId: string) => void
  onDragStart: (taskId: string) => void
  onDragEnd: () => void
}

/** List row: [checkbox] priority · id · status · title … labels · assignee · created. */
export function TaskRow({ task, statuses, labels, users, assignees, selected, dragging, onOpen, onToggleSelect, onDragStart, onDragEnd }: TaskRowProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const status = statuses.find((s) => s.id === task.statusId)
  const options = projectStatuses(statuses, task.projectId)
  return (
    <div
      className="group/row flex min-h-10 w-full min-w-0 cursor-pointer items-center gap-2 border-b border-border px-3 py-1.5 text-left transition-colors hover:bg-foreground/[0.02] data-[dragging]:bg-muted data-[selected]:bg-primary/10 max-[480px]:gap-1.5"
      data-selected={selected || undefined}
      data-dragging={dragging || undefined}
      role="button"
      tabIndex={0}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/task-id', task.id)
        onDragStart(task.id)
      }}
      onDragEnd={onDragEnd}
      onClick={() => onOpen(task.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target === e.currentTarget) onOpen(task.id)
      }}
    >
      <div className="relative -my-1.5 -ml-3 flex w-7 shrink-0 cursor-pointer self-stretch" onClick={(e) => e.stopPropagation()}>
        {/* the ::after overlay stretches the hit area over the whole 28px strip (the old label click target) */}
        <Checkbox
          checked={selected}
          aria-label={`Select ${task.identifier}`}
          onCheckedChange={() => onToggleSelect(task.id)}
          className="absolute top-1/2 left-3 size-4 -translate-y-1/2 cursor-pointer rounded border-foreground/20 bg-background opacity-0 transition-opacity group-hover/row:opacity-100 group-data-[selected]/row:opacity-100 after:-top-3 after:-bottom-3 after:-left-3 after:right-0 focus-visible:opacity-100 dark:bg-background"
        />
      </div>
      <PriorityPicker task={task} />
      <span className="w-[72px] shrink-0 text-xs whitespace-nowrap text-muted-foreground/70 tabular-nums max-[480px]:hidden">{task.identifier}</span>
      <div onClick={(e) => e.stopPropagation()}>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button variant="ghost" size="icon-sm" className="size-[22px]" aria-label={`Status: ${status?.name ?? 'None'}`}>
                <TaskStatusIcon status={status} />
              </Button>
            }
          />
          <DropdownMenuContent className={MENU}>
            {options.map((option) => (
              <DropdownMenuItem
                key={option.id}
                className={OPTION}
                data-selected={option.id === task.statusId || undefined}
                onClick={() => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })}
              >
                <TaskStatusIcon status={option} />
                {option.name}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium"><LinkifiedText text={task.title || 'Untitled'} /></span>
      {task.labels.length > 0 ? (
        <span className="flex shrink-0 gap-1 max-[1099px]:hidden">
          {task.labels.map((labelId) => {
            const label = labels.find((item) => item.id === labelId)
            return label ? <Badge key={label.id} variant="outline" className={PILL}><span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />{label.name}</Badge> : null
          })}
        </span>
      ) : null}
      <div onClick={(e) => e.stopPropagation()}>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                type="button"
                variant="ghost"
                className="inline-flex h-auto cursor-pointer rounded-none border-0 bg-transparent p-0 hover:bg-transparent aria-expanded:bg-transparent dark:hover:bg-transparent"
                aria-label={assignees.length > 0 ? `Assignees: ${assignees.map((user) => user.name).join(', ')}` : 'Assign task'}
              >
                <UserAvatarStack users={assignees} size={18} />
              </Button>
            }
          />
          <DropdownMenuContent align="end" className={MENU}>
            <DropdownMenuGroup className="flex flex-col gap-px">
              <DropdownMenuLabel className={HEADING}>Assignees</DropdownMenuLabel>
              {users.map((user) => {
                const active = task.assigneeIds.includes(user.id)
                return (
                  <DropdownMenuCheckboxItem
                    key={user.id}
                    className={CHECK_OPTION}
                    checked={active}
                    closeOnClick
                    onCheckedChange={() => updateTask.mutate({
                      taskId: task.id,
                      body: {
                        expected_version: task.version,
                        assignee_ids: active
                          ? task.assigneeIds.filter((id) => id !== user.id)
                          : [...task.assigneeIds, user.id],
                      },
                    })}
                  >
                    <UserAvatar user={user} size={16} />
                    {user.name}
                  </DropdownMenuCheckboxItem>
                )
              })}
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <span className="min-w-[44px] shrink-0 text-right text-xs text-muted-foreground/70 tabular-nums max-[480px]:hidden">{shortDate(task.createdAt)}</span>
      {updateTask.isError ? <span role="alert" className="text-xs text-destructive">Status update failed. <Button variant="ghost" onClick={(event) => { event.stopPropagation(); if (updateTask.variables) updateTask.mutate(updateTask.variables) }}>Retry</Button></span> : null}
    </div>
  )
}
