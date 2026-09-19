import { X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { UserAvatar, UserAvatarStack } from '../../../components/ui/UserAvatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { projectStatuses } from '../../../components/workspace/taskMeta'
import { shortDate } from '../../../lib/format'
import type { Task, TaskStatusDef, User } from '../api/models'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { useUpdateTask } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { PriorityPicker } from './PriorityPicker'
import { LinkifiedText } from './LinkifiedText'

const PILL = 'inline-flex h-[22px] items-center gap-1.5 rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const MENU = 'flex min-w-[180px] flex-col gap-px p-1'
const OPTION =
  'group flex w-full min-h-8 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent data-[selected]:bg-accent data-[selected]:font-medium'

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
      <label className="relative -my-1.5 -ml-3 flex w-7 shrink-0 cursor-pointer self-stretch" onClick={(e) => e.stopPropagation()}>
        <input type="checkbox" checked={selected} aria-label={`Select ${task.identifier}`} onChange={() => onToggleSelect(task.id)} className="peer absolute inset-0 z-[1] m-0 cursor-pointer appearance-none opacity-0" />
        <span className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 rounded border border-foreground/20 bg-background opacity-0 transition-opacity peer-checked:border-primary peer-checked:bg-primary peer-focus-visible:opacity-100 group-hover/row:opacity-100 group-data-[selected]/row:opacity-100" />
        <svg className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 scale-75 text-primary-foreground opacity-0 transition peer-checked:scale-100 peer-checked:opacity-100" viewBox="0 0 12 12" fill="none" aria-hidden="true">
          <path d="M2.5 6.5 5 9l4.5-6" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </label>
      <PriorityPicker task={task} />
      <span className="w-[72px] shrink-0 text-xs whitespace-nowrap text-muted-foreground/70 tabular-nums max-[480px]:hidden">{task.identifier}</span>
      <div onClick={(e) => e.stopPropagation()}>
        <Dropdown
          trigger={() => (
            <Button variant="ghost" size="icon-sm" className="size-[22px]" aria-label={`Status: ${status?.name ?? 'None'}`}>
              <TaskStatusIcon status={status} />
            </Button>
          )}
        >
          {(close) => (
            <div className={MENU}>
              {options.map((option) => (
                <button
                  key={option.id}
                  className={OPTION}
                  data-selected={option.id === task.statusId || undefined}
                  onClick={() => {
                    updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, status_id: option.id } })
                    close()
                  }}
                >
                  <TaskStatusIcon status={option} />
                  {option.name}
                </button>
              ))}
            </div>
          )}
        </Dropdown>
      </div>
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium"><LinkifiedText text={task.title || 'Untitled'} /></span>
      {task.labels.length > 0 ? (
        <span className="flex shrink-0 gap-1 max-[1099px]:hidden">
          {task.labels.map((labelId) => {
            const label = labels.find((item) => item.id === labelId)
            return label ? <span key={label.id} className={PILL}><span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />{label.name}</span> : null
          })}
        </span>
      ) : null}
      <div onClick={(e) => e.stopPropagation()}>
        <Dropdown
          align="right"
          trigger={() => (
            <button
              type="button"
              className="inline-flex cursor-pointer border-0 bg-transparent p-0"
              aria-label={assignees.length > 0 ? `Assignees: ${assignees.map((user) => user.name).join(', ')}` : 'Assign task'}
            >
              <UserAvatarStack users={assignees} size={18} />
            </button>
          )}
        >
          {(close) => (
            <div className={MENU}>
              <div className="px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase">Assignees</div>
              {users.map((user) => {
                const active = task.assigneeIds.includes(user.id)
                return (
                  <button
                    key={user.id}
                    className={OPTION}
                    data-selected={active || undefined}
                    aria-pressed={active}
                    onClick={() => {
                      updateTask.mutate({
                        taskId: task.id,
                        body: {
                          expected_version: task.version,
                          assignee_ids: active
                            ? task.assigneeIds.filter((id) => id !== user.id)
                            : [...task.assigneeIds, user.id],
                        },
                      })
                      close()
                    }}
                  >
                    <UserAvatar user={user} size={16} />
                    {user.name}
                    {active ? <X className="ml-auto size-3.5 shrink-0 text-muted-foreground/70 group-hover:text-foreground" aria-hidden="true" /> : null}
                  </button>
                )
              })}
            </div>
          )}
        </Dropdown>
      </div>
      <span className="min-w-[44px] shrink-0 text-right text-xs text-muted-foreground/70 tabular-nums max-[480px]:hidden">{shortDate(task.createdAt)}</span>
      {updateTask.isError ? <span role="alert" className="text-xs text-destructive">Status update failed. <Button variant="ghost" onClick={(event) => { event.stopPropagation(); if (updateTask.variables) updateTask.mutate(updateTask.variables) }}>Retry</Button></span> : null}
    </div>
  )
}
