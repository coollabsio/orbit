import type { Crumb } from '../../components/shell/TopbarBreadcrumb'
import type { Task } from './api/models'

/**
 * Crumbs for one task: its parent (if any) then the task itself. The shell owns
 * the `Orbit` root. `tasks` is whatever is already loaded (the list page plus a
 * parent fetched on demand); `parentPending` says the parent is still loading.
 */
export function taskCrumbs(task: Task, tasks: Task[], parentPending = false): Crumb[] {
  const self: Crumb = { label: task.identifier }
  if (!task.parentId) return [self]
  const parent = tasks.find((candidate) => candidate.id === task.parentId)
  if (parent) return [{ label: parent.identifier, to: `/tasks/${parent.id}` }, self]
  // Still loading: a neutral placeholder, never a premature "In trash".
  if (parentPending) return [{ label: '…', muted: true }, self]
  // A soft-deleted parent keeps its children; render that state, not a dead link.
  return [{ label: 'In trash', muted: true }, self]
}
