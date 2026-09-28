import type { CreateTaskBody } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES } from '@/features/tasks/api/tasks'
import { defaultStatusOf } from '@/features/tasks/taskMeta'

/** Success toast after a parent change; `parentIdentifier` null = the tasks were detached. */
export function parentToastMessage(identifiers: string[], parentIdentifier: string | null): string {
  const one = identifiers.length === 1
  if (parentIdentifier === null) return one ? `${identifiers[0]} is no longer a sub-issue` : `${identifiers.length} tasks are no longer sub-issues`
  return one ? `${identifiers[0]} is now a sub-issue of ${parentIdentifier}` : `${identifiers.length} tasks are now sub-issues of ${parentIdentifier}`
}

/** Error toast for a failed parent change: the server's `parent_cycle` / `parent_invalid` refusals explain themselves. */
export function parentErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'parent_cycle') return 'A task can’t be a sub-issue of itself or of its own sub-issues.'
    if (error.code === 'parent_invalid') return 'That parent task isn’t available anymore.'
  }
  if (error instanceof BulkTaskLimitError) return `Select ${MAX_BULK_TASK_UPDATES} or fewer tasks and try again.`
  return 'Couldn’t change the parent.'
}

/** `subject` is an identifier, or a count for the bulk picker. */
export function parentPickerTitle(subject: string | number): string {
  return typeof subject === 'number' ? `Set parent of ${subject} tasks…` : `Set parent of ${subject}…`
}

const sameSet = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((id) => b.includes(id))

/**
 * Linear's rules for a new sub-issue (spec §7.6): the parent's project and priority, that project's default status,
 * no labels. The parent's assignees are copied only when the current user is one of them, or when every existing
 * sub-issue already has exactly the parent's assignees. Null when the project has no status.
 */
export function subIssueDefaults(parent: Pick<Task, 'id' | 'projectId' | 'priority' | 'assigneeIds'>, { statuses, currentUserId, siblings }: {
  statuses: TaskStatusDef[]; currentUserId: string; siblings: Array<Pick<Task, 'assigneeIds'>>
}): Omit<CreateTaskBody, 'title'> | null {
  const status = defaultStatusOf(statuses, parent.projectId)
  if (!status) return null
  const inherit = parent.assigneeIds.length > 0
    && (parent.assigneeIds.includes(currentUserId) || (siblings.length > 0 && siblings.every((sibling) => sameSet(sibling.assigneeIds, parent.assigneeIds))))
  return { project_id: parent.projectId, status_id: status.id, priority: parent.priority, assignee_ids: inherit ? [...parent.assigneeIds] : [], parent_task_id: parent.id }
}

/** Breadcrumb folding: up to three ancestors show in full; more keep the root and the direct parent around a menu. */
export function breadcrumbParts<T>(ancestors: T[]): { head: T[]; hidden: T[]; tail: T[] } {
  if (ancestors.length <= 3) return { head: [...ancestors], hidden: [], tail: [] }
  return { head: ancestors.slice(0, 1), hidden: ancestors.slice(1, -1), tail: ancestors.slice(-1) }
}

type SubIssueCountNode = { id: string; sub_issue_count?: number | null }

/**
 * How many live sub-issues sit below a task (at every level), from the loaded levels. `direct` is the task's own
 * count; `childrenOf(id)` gives a task's loaded sub-issues, or undefined when that level is not loaded. Exact when
 * every level that has sub-issues is loaded; otherwise only the direct count is known (`exact: false`).
 */
export function descendantCount(direct: number, childrenOf: (id: string) => readonly SubIssueCountNode[] | undefined, taskId: string): { total: number; exact: boolean } {
  if (direct === 0) return { total: 0, exact: true }
  let total = 0
  const pending = [taskId]
  while (pending.length > 0) {
    const children = childrenOf(pending.pop()!)
    if (!children) return { total: direct, exact: false }
    total += children.length
    for (const child of children) if ((child.sub_issue_count ?? 0) > 0) pending.push(child.id)
  }
  return { total, exact: true }
}

/** The trash confirmation's body: says how many sub-issues go to trash too (spec §5 trashes the whole subtree). */
export function trashConfirmDescription({ total, exact }: { total: number; exact: boolean }): string {
  if (total === 0) return 'You can restore this task from trash later.'
  const count = `${total} sub-issue${total === 1 ? '' : 's'}`
  const moved = exact ? `This also moves ${count} to trash.` : `This also moves its ${count} and everything nested under them to trash.`
  return `${moved} You can restore them from trash later.`
}

/** "+2 sub-issues" beside a trashed parent; null when it went to trash alone. */
export function trashedSubIssuesLabel(count: number): string | null {
  return count > 0 ? `+${count} sub-issue${count === 1 ? '' : 's'}` : null
}
