import { createContext } from 'react'
import type { StatusCategory } from '../../features/tasks/api/models'

export type TaskChipLookup = (
  identifier: string,
) => { title: string; statusCategory: StatusCategory; taskId?: string } | undefined

/**
 * The live chip lookup reaches `taskMention` through context rather than a
 * closure, so the static node mapping is a set of module-level components with
 * stable identities — a new lookup never remounts the rendered document.
 */
export const TaskChipContext = createContext<TaskChipLookup | undefined>(undefined)
