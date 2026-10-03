import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import type { Task } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { toggleAssignee } from '@/features/tasks/pickerLib'
import { useTaskPickerUpdate } from '@/features/tasks/useTaskPickerUpdate'

/**
 * Avatar stack that opens a checkbox menu of the workspace members to change the assignees in place (list rows, board
 * cards, timeline and sub-issue rows). Each toggle saves and closes the menu. Without assignees the stack shows the
 * empty avatar; `className` (on the wrapper) can hide it until the row is hovered.
 */
export function AssigneePicker({ task, users, size = 18, max = 2, align = 'end', className }: {
  task: Pick<Task, 'id' | 'version' | 'assigneeIds'>
  users: User[]
  size?: number
  /** Most avatars shown before the "+n" count. */
  max?: number
  align?: 'start' | 'end'
  className?: string
}) {
  const save = useTaskPickerUpdate(task, 'Assignee update failed.')
  const assignees = users.filter((user) => task.assigneeIds.includes(user.id))
  return (
    <div className={cn('flex shrink-0', className)} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger
          render={
            <Button
              type="button"
              variant="ghost"
              className="h-auto rounded-full p-0"
              aria-label={assignees.length > 0 ? `Assignees: ${assignees.map((user) => user.name).join(', ')}` : 'Assign task'}
            >
              <UserAvatarStack users={assignees} size={size} max={max} />
            </Button>
          }
        />
        <DropdownMenuContent align={align} className="w-auto min-w-45">
          <DropdownMenuGroup>
            <DropdownMenuLabel>Assignees</DropdownMenuLabel>
            {users.map((user) => (
              <DropdownMenuCheckboxItem
                key={user.id}
                checked={task.assigneeIds.includes(user.id)}
                closeOnClick
                onCheckedChange={() => save({ assignee_ids: toggleAssignee(task.assigneeIds, user.id) })}
              >
                <UserAvatar user={user} size={16} />
                {user.name}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}
