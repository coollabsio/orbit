import { ApiProblem } from '../../../api/problem'

export function isTaskVersionConflict(error: unknown): boolean {
  return error instanceof ApiProblem && error.status === 409 && ['task_conflict', 'conflict', 'restore_conflict'].includes(error.code)
}
