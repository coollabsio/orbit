import type { Problem } from './generated/types.gen'

export type ProblemDocument = Problem

export interface FieldError {
  path: string[]
  message: string
}

export class ApiProblem extends Error {
  readonly problem: ProblemDocument

  constructor(problem: ProblemDocument) {
    super(problem.detail)
    this.name = 'ApiProblem'
    this.problem = problem
  }

  get status() {
    return this.problem.status
  }

  get code() {
    return this.problem.code
  }

  get detail() {
    return this.problem.detail
  }
}

export function fieldErrors(error: ApiProblem): FieldError[] {
  return Object.entries(error.problem.errors ?? {})
    .sort(([left], [right]) => left.localeCompare(right))
    .flatMap(([field, messages]) =>
      messages.map((message) => ({
        path: field.replaceAll('[', '.').replaceAll(']', '').split('.').filter(Boolean),
        message,
      })),
    )
}

export async function parseProblem(response: Response): Promise<ApiProblem> {
  const contentType = response.headers.get('content-type') ?? ''
  if (contentType.includes('json')) {
    try {
      const value: unknown = await response.clone().json()
      if (isProblem(value)) return new ApiProblem(value)
    } catch {
      // Fall through to a safe generic error.
    }
  }

  return new ApiProblem({
    type: 'about:blank',
    title: 'Request failed',
    status: response.status,
    code: 'http_error',
    detail: 'The server could not complete the request.',
    instance: new URL(response.url || 'http://localhost/').pathname,
    request_id: response.headers.get('x-request-id') ?? 'unknown',
  })
}

function isProblem(value: unknown): value is ProblemDocument {
  if (!value || typeof value !== 'object') return false
  const problem = value as Record<string, unknown>
  return (
    typeof problem.type === 'string' &&
    typeof problem.title === 'string' &&
    typeof problem.status === 'number' &&
    typeof problem.code === 'string' &&
    typeof problem.detail === 'string' &&
    typeof problem.instance === 'string' &&
    typeof problem.request_id === 'string'
  )
}
