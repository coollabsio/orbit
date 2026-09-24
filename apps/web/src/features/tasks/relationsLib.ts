import type { TaskRelationRecord } from '@/api/generated/types.gen'
import type { Task } from '@/features/tasks/api/models'

/** What the picker chooses a task for. `blocked_by` is stored server-side as "the other task blocks this one". */
export type RelationKind = 'duplicate' | 'blocks' | 'blocked_by' | 'related'
export type RelationGroupKey = 'duplicate_of' | 'blocked_by' | 'blocks' | 'related' | 'duplicated_by'

export const RELATION_GROUP_LABEL: Record<RelationGroupKey, string> = {
  duplicate_of: 'Duplicate of',
  blocked_by: 'Blocked by',
  blocks: 'Blocks',
  related: 'Related',
  duplicated_by: 'Duplicated by',
}

const GROUP_ORDER: RelationGroupKey[] = ['duplicate_of', 'blocked_by', 'blocks', 'related', 'duplicated_by']

export interface RelationGroup {
  key: RelationGroupKey
  label: string
  relations: TaskRelationRecord[]
}

/** Section of a relation seen from the viewed task; null for types this client does not know. */
export function relationGroupOf({ type, direction }: Pick<TaskRelationRecord, 'type' | 'direction'>): RelationGroupKey | null {
  if (type === 'duplicate') return direction === 'outgoing' ? 'duplicate_of' : 'duplicated_by'
  if (type === 'blocks') return direction === 'outgoing' ? 'blocks' : 'blocked_by'
  if (type === 'related') return 'related'
  return null
}

/** Non-empty groups in the fixed order, oldest relation first inside a group. */
export function groupRelations(relations: TaskRelationRecord[]): RelationGroup[] {
  return GROUP_ORDER.flatMap((key) => {
    const own = relations
      .filter((relation) => relationGroupOf(relation) === key)
      .sort((a, b) => a.created_at.localeCompare(b.created_at))
    return own.length > 0 ? [{ key, label: RELATION_GROUP_LABEL[key], relations: own }] : []
  })
}

/** Other-side task ids of every known relation (excluded from the blocks/related picker). */
export function relatedTaskIds(relations: TaskRelationRecord[]): string[] {
  return relations.filter((relation) => relationGroupOf(relation) !== null).map((relation) => relation.task.id)
}

/** `subject` is an identifier, or a count for the bulk duplicate picker. */
export function pickerTitle(kind: RelationKind, subject: string | number): string {
  switch (kind) {
    case 'duplicate':
      return typeof subject === 'number' ? `Mark ${subject} tasks as duplicate of…` : `Mark ${subject} as duplicate of…`
    case 'blocks':
      return `${subject} blocks…`
    case 'blocked_by':
      return `${subject} is blocked by…`
    case 'related':
      return `Relate ${subject} to…`
  }
}

export const ADD_RELATION_OPTIONS: Array<{ kind: RelationKind; label: string }> = [
  { kind: 'blocks', label: 'Blocks…' },
  { kind: 'blocked_by', label: 'Blocked by…' },
  { kind: 'related', label: 'Related to…' },
  { kind: 'duplicate', label: 'Mark as duplicate of…' },
]

/** Deduped picker rows: identifier, title or description contain the query; excluded ids and (optionally) duplicates dropped. */
export function pickerCandidates({ tasks, query, excludeIds, excludeDuplicates, limit = 20 }: {
  tasks: Task[]
  query: string
  excludeIds: readonly string[]
  excludeDuplicates: boolean
  limit?: number
}): Task[] {
  const needle = query.trim().toLowerCase()
  const skip = new Set(excludeIds)
  const seen = new Set<string>()
  const result: Task[] = []
  for (const task of tasks) {
    if (seen.has(task.id) || skip.has(task.id)) continue
    seen.add(task.id)
    if (excludeDuplicates && task.duplicateOf) continue
    if (needle && !`${task.identifier} ${task.title} ${task.description}`.toLowerCase().includes(needle)) continue
    result.push(task)
    if (result.length === limit) break
  }
  return result
}

export function duplicateToastMessage(count: number, targetIdentifier: string): string {
  return count === 1
    ? `Marked as duplicate of ${targetIdentifier}`
    : `Marked ${count} tasks as duplicate of ${targetIdentifier}`
}

export function duplicateErrorMessage(verb: 'mark' | 'unmark', error: unknown): string {
  const reason = error instanceof Error && error.message ? ` ${error.message}` : ''
  return `Couldn't ${verb} as duplicate.${reason}`
}
