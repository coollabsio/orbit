import { cn } from 'cn'
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
import { BlockedIndicator } from './BlockedIndicator'
import { projectStatuses } from '@/features/tasks/taskMeta'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { LabelRecord } from '@/api/generated/types.gen'
import type { TaskProperty } from '@/features/views/viewState'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { PriorityPicker } from './PriorityPicker'
import { LinkifiedText } from './LinkifiedText'
import { DateStamp, DueDateChip, ProjectChip } from './TaskPropertyChips'

const PILL = 'inline-flex h-[22px] items-center gap-1.5 overflow-visible rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`
/** Multi-select rows keep room on the right for the checked indicator. */
const CHECK_OPTION =
  `group min-h-8 cursor-pointer gap-2 py-1.5 pr-8 pl-2 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-checked:bg-accent data-checked:font-medium`
const HEADING = 'px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase'
/** 2px insertion line on the row edge while a manual-order drag hovers it; absolute, so nothing shifts. */
const DROP_LINE =
  'data-[drop-edge]:before:pointer-events-none data-[drop-edge]:before:absolute data-[drop-edge]:before:inset-x-0 data-[drop-edge]:before:z-[1] data-[drop-edge]:before:h-0.5 data-[drop-edge]:before:bg-primary data-[drop-edge=top]:before:-top-px data-[drop-edge=bottom]:before:-bottom-px'

interface TaskRowProps {
  task: Task
  statuses: TaskStatusDef[]
  labels: LabelRecord[]
  users: User[]
  assignees: User[]
  project: Project | undefined
  /** Only these properties render (`display.properties`). */
  properties: TaskProperty[]
  selected: boolean
  dragging: boolean
  /** False when no drop could change anything (e.g. grouped by project with a non-manual order). */
  draggable: boolean
  /** Where the insertion line shows during a manual-order drag. */
  dropEdge: 'top' | 'bottom' | null
  onOpen: (taskId: string) => void
  onToggleSelect: (taskId: string) => void
  onDragStart: (taskId: string) => void
  onDragEnd: () => void
  /** Duplicate needs a canonical task: the list opens its picker. */
  onRequestDuplicate: (task: Task) => void
}

/** List row: [checkbox] priority · id · status · title … labels · project · due · assignee · created · updated. */
export function TaskRow({ task, statuses, labels, users, assignees, project, properties, selected, dragging, draggable, dropEdge, onOpen, onToggleSelect, onDragStart, onDragEnd, onRequestDuplicate }: TaskRowProps) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const status = statuses.find((s) => s.id === task.statusId)
  const options = projectStatuses(statuses, task.projectId)
  const has = (property: TaskProperty) => properties.includes(property)
  return (
    <div
      className={cn(
        'group/row relative flex min-h-10 w-full min-w-0 cursor-pointer items-center gap-2 border-b border-border px-3 py-1.5 text-left transition-colors hover:bg-foreground/[0.02] data-[dragging]:bg-muted data-[dragging]:opacity-50 data-[selected]:bg-primary/10 max-[480px]:gap-1.5',
        DROP_LINE,
      )}
      data-task-row
      data-selected={selected || undefined}
      data-dragging={dragging || undefined}
      data-drop-edge={dropEdge ?? undefined}
      role="button"
      tabIndex={0}
      draggable={draggable}
      onDragStart={draggable ? (e) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/task-id', task.id)
        onDragStart(task.id)
      } : undefined}
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
      {has('priority') ? <PriorityPicker task={task} /> : null}
      {/* the blocked mark lives inside the fixed id column so titles stay aligned; on phones only the mark shows */}
      {has('id') ? (
        <span className={cn('inline-flex w-[72px] shrink-0 items-center gap-1 text-xs whitespace-nowrap text-muted-foreground/70 tabular-nums max-[480px]:w-auto', !task.blocked && 'max-[480px]:hidden')}>
          <span className="whitespace-nowrap max-[480px]:hidden">{task.identifier}</span>
          {task.blocked ? <BlockedIndicator /> : null}
        </span>
      ) : null}
      {has('status') ? (
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
                  onClick={() => option.category === 'duplicate'
                    ? onRequestDuplicate(task)
                    : updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })}
                >
                  <TaskStatusIcon status={option} />
                  {option.name}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      ) : null}
      {!has('id') && task.blocked ? <BlockedIndicator /> : null}
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium"><LinkifiedText text={task.title || 'Untitled'} /></span>
      {has('labels') && task.labels.length > 0 ? (
        <span className="flex shrink-0 gap-1 max-[1099px]:hidden">
          {task.labels.map((labelId) => {
            const label = labels.find((item) => item.id === labelId)
            return label ? <Badge key={label.id} variant="outline" className={PILL}><span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />{label.name}</Badge> : null
          })}
        </span>
      ) : null}
      {has('project') ? <ProjectChip project={project} className="text-xs max-[1099px]:hidden" /> : null}
      {has('due_date') ? <DueDateChip task={task} status={status} className="text-xs max-[640px]:hidden" /> : null}
      {has('assignee') ? (
        <div className="flex" onClick={(e) => e.stopPropagation()}>
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
      ) : null}
      {has('created') ? <DateStamp property="created" iso={task.createdAt} className="min-w-[44px] text-right text-xs text-muted-foreground/70 max-[480px]:hidden" /> : null}
      {has('updated') ? <DateStamp property="updated" iso={task.updatedAt} className="min-w-[44px] text-right text-xs text-muted-foreground/70 max-[480px]:hidden" /> : null}
      {updateTask.isError ? <span role="alert" className="text-xs text-destructive">Status update failed. <Button variant="ghost" onClick={(event) => { event.stopPropagation(); if (updateTask.variables) updateTask.mutate(updateTask.variables) }}>Retry</Button></span> : null}
    </div>
  )
}
