import { UserAvatar } from '@/components/common/UserAvatar'
import { PriorityIcon } from '@/features/tasks/components/PriorityIcon'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { PRIORITY_ORDER, statusKeyOf } from '@/features/tasks/taskMeta'
import type { GroupContext, TaskGroup } from '../grouping'

const DOT = 'size-2 shrink-0 rounded-full'

/** The glyph in front of a group name: status icon, priority bars, avatar, or a project/label colour dot. */
export function GroupIcon({ group, context }: { group: Pick<TaskGroup, 'field' | 'value'>; context: GroupContext }) {
  switch (group.field) {
    case 'status':
      return <TaskStatusIcon status={context.statuses.find((status) => statusKeyOf(status) === group.value)} />
    case 'priority':
      return <PriorityIcon priority={PRIORITY_ORDER.find((priority) => priority === group.value) ?? 'none'} />
    case 'assignee': {
      const member = context.members.find((user) => user.id === group.value)
      return <UserAvatar user={member} size={16} name={member ? undefined : '—'} />
    }
    case 'project': {
      const color = context.projects.find((project) => project.id === group.value)?.color
      return <span aria-hidden className={DOT} style={{ background: color ?? 'var(--muted-foreground)' }} />
    }
    case 'label': {
      const color = context.labels.find((label) => label.id === group.value)?.color
      return color
        ? <span aria-hidden className={DOT} style={{ background: color }} />
        : <span aria-hidden className={`${DOT} border border-dashed border-muted-foreground/60`} />
    }
    default:
      return null
  }
}
