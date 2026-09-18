import { createContext } from 'react'
import type { StatusCategory } from '../../features/tasks/api/models'

/** The live values behind a chip. The document itself only caches the identifier. */
export interface ResolvedChip {
  title: string
  statusCategory: StatusCategory
  taskId?: string
  /** The status colour; omitted when the status definition is not loaded. */
  statusColor?: string
}

export type TaskChipLookup = (identifier: string) => ResolvedChip | undefined

/**
 * The live chip lookup reaches `taskMention` through context rather than a
 * closure, so the static node mapping is a set of module-level components with
 * stable identities — a new lookup never remounts the rendered document.
 */
export const TaskChipContext = createContext<TaskChipLookup | undefined>(undefined)
