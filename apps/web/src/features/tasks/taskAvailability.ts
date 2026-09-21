import { ApiProblem } from '@/api/problem'

export function taskUnavailableDescription(error: unknown): string {
  if (error instanceof ApiProblem && error.status === 404) {
    return 'This task may have been deleted, or you may no longer have access to it.'
  }

  return 'The server could not load this task.'
}
