// The card that says what a reference in a text is (a person, a task, a channel) while the pointer rests on it. It only
// informs: it takes no focus and no clicks, so typing and selecting go on under it.
import type { ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { cn } from 'cn'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useLinkedTaskLookup } from '@/features/tasks/useLinkedTask'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { shortDate } from '@/lib/format'
import { UserAvatar, UserAvatarStack } from './UserAvatar'

const CARD_WIDTH = 288
const GAP = 6
const EDGE = 8

/** The floating card, above the hovered text (`below` where something else already sits above it). */
export function ReferencePreviewCard({ rect, placement = 'above', children }: { rect: DOMRect; placement?: 'above' | 'below'; children: ReactNode }) {
  const left = Math.min(Math.max(EDGE, rect.left), window.innerWidth - CARD_WIDTH - EDGE)
  const position = placement === 'above' ? { bottom: window.innerHeight - rect.top + GAP } : { top: rect.bottom + GAP }
  return createPortal(
    <div
      role="tooltip"
      data-slot="reference-preview"
      data-placement={placement}
      style={{ left, width: CARD_WIDTH, ...position }}
      className={cn(
        'pointer-events-none fixed z-50 rounded-lg border bg-popover p-3 text-[0.8125rem] text-popover-foreground shadow-lg',
        'animate-in fade-in-0 duration-150 ease-out motion-reduce:animate-none',
        placement === 'above' ? 'slide-in-from-bottom-1' : 'slide-in-from-top-1',
      )}
    >
      {children}
    </div>,
    document.body,
  )
}

/** A line under a preview: what happens to the reference (who is notified, why it stays text). */
export function PreviewNote({ tone = 'muted', children }: { tone?: 'muted' | 'warning'; children: ReactNode }) {
  return <p className={cn('mt-2 border-t pt-2 text-xs', tone === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-muted-foreground')}>{children}</p>
}

/** A member: avatar, name, job title or handle. */
export function PersonPreview({ userId, fallbackName, note }: { userId: string; fallbackName?: string; note?: ReactNode }) {
  const { workspace } = useWorkspace()
  const member = useMembers(workspace.id).data?.find((candidate) => candidate.id === userId)
  const name = member?.name ?? fallbackName ?? 'Unknown user'
  return (
    <>
      <div className="flex items-center gap-2.5">
        <UserAvatar user={member ? { name: member.name, color: member.color, avatarUrl: member.avatarUrl } : undefined} name={name} size={36} />
        <div className="min-w-0">
          <div className="truncate font-medium">{name}</div>
          <div className="truncate text-xs text-muted-foreground">{member ? member.title || `@${member.handle}` : 'No longer a member'}</div>
        </div>
      </div>
      {note}
    </>
  )
}

/**
 * A task: status, ID, project, title, assignees and due date; or why the ID names no task (`missingNote` adds what then
 * happens to the reference).
 */
export function TaskPreview({ taskRef, note, missingNote }: { taskRef: string; note?: ReactNode; missingNote?: string }) {
  const { workspace } = useWorkspace()
  const members = useMembers(workspace.id).data ?? []
  const { linked, missing, failed } = useLinkedTaskLookup(taskRef)
  if (!linked) {
    if (failed) return <div className="text-muted-foreground">Could not load {taskRef}.</div>
    return missing ? (
      <>
        <div className="font-medium">No task {taskRef}</div>
        <PreviewNote>It does not exist, is in the trash, or you cannot see it.{missingNote ? ` ${missingNote}` : ''}</PreviewNote>
      </>
    ) : (
      <div className="text-muted-foreground">Loading {taskRef}…</div>
    )
  }
  const { task, project, status } = linked
  const assignees = task.assigneeIds.flatMap((id) => members.filter((member) => member.id === id))
  return (
    <>
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <span className="tabular-nums">{task.identifier}</span>
        {project ? <span className="truncate">in {project.name}</span> : null}
      </div>
      <div className="mt-1 line-clamp-2 font-medium">{task.title}</div>
      <div className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
        <TaskStatusIcon status={status} size={13} />
        <span className="truncate">{status?.name ?? 'No status'}</span>
        {task.dueAt ? <span className="shrink-0">Due {shortDate(task.dueAt)}</span> : null}
        {assignees.length > 0 ? (
          <span className="ml-auto shrink-0">
            <UserAvatarStack users={assignees.map((member) => ({ id: member.id, name: member.name, color: member.color, avatarUrl: member.avatarUrl }))} size={18} />
          </span>
        ) : null}
      </div>
      {note}
    </>
  )
}
