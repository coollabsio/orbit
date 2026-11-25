import { useState } from 'react'
import { toast } from 'sonner'
import { Edit as Pencil, MoreH as MoreHorizontal, Trash as Trash2 } from 'reicon-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { confirmAction } from '@/components/common/confirmAction'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { Task, TaskViewState, TimeEntry } from '@/features/tasks/api/models'
import { useDeleteTimeEntry } from '@/features/tasks/api/timeEntries'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { formatDuration, workedOnLabel } from '@/features/tasks/timeLib'
import { TimeEntryDialog } from './TimeEntryDialog'

/** A time entry in the activity feed: who logged how long for which day, and the note. */
export function TimeEntryItem({ task, state, entry }: { task: Task; state: TaskViewState; entry: TimeEntry }) {
  const { workspace } = useWorkspace()
  const deleteEntry = useDeleteTimeEntry(workspace.id, task.id)
  const [editing, setEditing] = useState(false)
  const person = state.users.find((user) => user.id === entry.user_id)
  const name = person?.name ?? 'Someone'
  const duration = formatDuration(entry.minutes)

  const remove = async () => {
    if (!await confirmAction({ title: 'Delete this time entry?', description: `${duration} on ${workedOnLabel(entry.worked_on)} is removed from the task's time.`, confirmLabel: 'Delete', danger: true })) return
    deleteEntry.mutate(entry.id, { onError: () => toast.error('The time entry could not be deleted.') })
  }

  return (
    <article data-slot="time-entry" className="group/time my-3 flex gap-2 rounded-[10px] border bg-card/50 px-3.5 py-2.5">
      <ProfileTrigger userId={person?.id} name={name} kind="avatar" tabIndex={-1} className="flex shrink-0 self-start">
        <UserAvatar user={person} size={20} name={name} />
      </ProfileTrigger>
      <div className="min-w-0 flex-1">
        <div className="flex min-h-5 items-center gap-1.5 text-[0.8125rem] text-muted-foreground">
          <span className="min-w-0 truncate">
            <ProfileTrigger userId={person?.id} name={name} className="font-semibold text-foreground">{name}</ProfileTrigger>
            {' logged '}
            <span className="font-medium text-foreground tabular-nums">{duration}</span>
            {' · '}
            <time dateTime={entry.worked_on}>{workedOnLabel(entry.worked_on)}</time>
          </span>
          {entry.billable ? <Badge variant="secondary">Billable</Badge> : null}
          {entry.can_edit ? (
            <DropdownMenu>
              <Tip label="Time entry actions">
                <DropdownMenuTrigger
                  render={
                    <Button variant="ghost" size="icon-sm" className="ml-auto size-6 text-muted-foreground/70 opacity-0 transition-opacity group-hover/time:opacity-100 group-focus-within/time:opacity-100 aria-expanded:opacity-100 max-[899px]:opacity-100" aria-label="Time entry actions">
                      <MoreHorizontal className="size-3.5" />
                    </Button>
                  }
                />
              </Tip>
              <DropdownMenuContent align="end" className="w-auto min-w-36">
                <DropdownMenuItem onClick={() => setEditing(true)}>
                  <Pencil className="size-3.5" />
                  Edit
                </DropdownMenuItem>
                <DropdownMenuItem variant="destructive" onClick={() => void remove()}>
                  <Trash2 className="size-3.5" />
                  Delete
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </div>
        {entry.note.trim() ? (
          <div className="mt-0.5 text-[0.8125rem] leading-[1.1875rem] text-foreground [overflow-wrap:anywhere]">
            {renderMarkdownBlocks(entry.note, entry.id)}
          </div>
        ) : null}
      </div>
      {editing ? <TimeEntryDialog task={task} users={state.users} currentUserId={state.currentUserId} entry={entry} onClose={() => setEditing(false)} /> : null}
    </article>
  )
}
