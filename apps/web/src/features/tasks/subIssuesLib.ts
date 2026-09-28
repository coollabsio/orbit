import { ApiProblem } from '@/api/problem'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES } from '@/features/tasks/api/tasks'

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
