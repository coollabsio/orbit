import { ChevronRight } from 'reicon-react'
import { cn } from 'cn'
import { UserAvatar } from '@/components/common/UserAvatar'
import { ColorDot } from '@/components/common/ColorDot'
import { PriorityIcon } from '@/features/tasks/components/PriorityIcon'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { PRIORITY_ORDER, statusKeyOf } from '@/features/tasks/taskMeta'
import type { GroupContext, TaskGroup } from '../grouping'

/**
 * The collapse chevron of group headers, swim lanes and tree rows (points right, turns 90° when open): 150ms rotation
 * with a strong ease-out; the content never animates its height. Reduced motion: no rotation.
 */
export function DisclosureChevron({ open, className }: { open: boolean; className?: string }) {
  return (
    <ChevronRight
      aria-hidden
      data-slot="disclosure-chevron"
      className={cn('size-3 transition-transform duration-150 ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:transition-none', open && 'rotate-90', className)}
    />
  )
}

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
      return <ColorDot color={color} className={cn('size-2', !color && 'bg-muted-foreground')} />
    }
    case 'label': {
      const color = context.labels.find((label) => label.id === group.value)?.color
      return color
        ? <ColorDot color={color} className="size-2" />
        : <span aria-hidden className="size-2 shrink-0 rounded-full border border-dashed border-muted-foreground/60" />
    }
    default:
      return null
  }
}
