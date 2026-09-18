import { apiClient } from '../../../api/client'
import { listTasks } from '../../../api/generated/sdk.gen'
import type { TaskRecord } from '../../../api/generated/types.gen'
import type { User } from '../../../features/tasks/api/models'

export const MAX_USER_SUGGESTIONS = 5
export const MAX_TASK_SUGGESTIONS = 8
export const SUGGESTION_DEBOUNCE_MS = 150

export type MentionItem =
  | { kind: 'user'; id: string; label: string; handle: string }
  | { kind: 'task'; id: string; identifier: string; title: string }

/**
 * One unified `@` menu. People are filtered locally from the member list the
 * task page already holds; issues arrive pre-ranked from the fts5-backed search
 * (identifier, then title, then body), so their order is preserved verbatim.
 */
export function buildMentionItems({
  query,
  members,
  tasks,
}: {
  query: string
  members: User[]
  tasks: TaskRecord[]
}): MentionItem[] {
  const needle = query.trim().toLowerCase()
  const people = members
    .filter(
      (member) =>
        needle === '' ||
        member.name.toLowerCase().includes(needle) ||
        member.handle.toLowerCase().includes(needle),
    )
    .slice(0, MAX_USER_SUGGESTIONS)
    .map((member): MentionItem => ({ kind: 'user', id: member.id, label: member.name, handle: member.handle }))
  const issues = tasks
    .slice(0, MAX_TASK_SUGGESTIONS)
    .map((task): MentionItem => ({ kind: 'task', id: task.id, identifier: task.identifier, title: task.title }))
  return [...people, ...issues]
}

/**
 * Debounced, fts5-backed issue lookup: `GET .../tasks?search=&sort=relevance`.
 * An empty query never reaches the network. A call superseded by a newer one
 * resolves to `[]` instead of hanging, so callers awaiting it are released —
 * they must ignore results for a query that is no longer current.
 *
 * Exported because `TaskMentionSearch` reuses this exact query: one search
 * implementation, two consumers.
 */
export function createTaskLookup(workspaceId: string) {
  let timer: ReturnType<typeof setTimeout> | undefined
  let release: (() => void) | undefined
  let inFlight: AbortController | undefined

  return (query: string): Promise<TaskRecord[]> => {
    if (timer) clearTimeout(timer)
    timer = undefined
    release?.()
    release = undefined
    inFlight?.abort()
    inFlight = undefined
    const search = query.trim()
    if (search === '') return Promise.resolve([])
    return new Promise((resolve) => {
      release = () => resolve([])
      timer = setTimeout(async () => {
        timer = undefined
        release = undefined
        const controller = new AbortController()
        inFlight = controller
        try {
          const { data } = await listTasks({
            client: apiClient,
            path: { workspace_id: workspaceId },
            query: { search, sort: 'relevance', limit: MAX_TASK_SUGGESTIONS },
            signal: controller.signal,
            throwOnError: true,
          })
          resolve(data?.items ?? [])
        } catch {
          resolve([])
        } finally {
          if (inFlight === controller) inFlight = undefined
        }
      }, SUGGESTION_DEBOUNCE_MS)
    })
  }
}
