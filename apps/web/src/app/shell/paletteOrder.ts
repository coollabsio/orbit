// Where the command palette puts each kind of result: the part of the app in view leads, a typed task ID puts tasks
// first, and a query that names a destination ("set" → Settings) puts navigation first.
import { looksLikeTaskId } from '@/features/tasks/taskReferences'

export type PaletteArea = 'tasks' | 'docs' | 'chat' | 'other'
export type PaletteSection = 'tasks' | 'docs' | 'chat' | 'navigation'

/** The part of the app a path belongs to. */
export function paletteArea(pathname: string): PaletteArea {
  if (/^\/(tasks|views|tasks-trash)(\/|$)/.test(pathname)) return 'tasks'
  if (/^\/docs(\/|$)/.test(pathname)) return 'docs'
  if (/^\/chat(\/|$)/.test(pathname)) return 'chat'
  return 'other'
}

const CONTENT_ORDER: Record<PaletteArea, PaletteSection[]> = {
  tasks: ['tasks', 'docs', 'chat'],
  docs: ['docs', 'tasks', 'chat'],
  chat: ['chat', 'tasks', 'docs'],
  other: ['tasks', 'docs', 'chat'],
}

/** A word of a destination's name starts with the query: "set" names Settings, "trash" names Task trash. */
export function namesDestination(name: string, query: string): boolean {
  const needle = query.trim().toLowerCase()
  return needle.length > 0 && name.toLowerCase().split(/\s+/).some((word) => word.startsWith(needle))
}

/** The order of the result sections for a query typed in `area`. */
export function sectionOrder(area: PaletteArea, query: string, navigationNamed: boolean): PaletteSection[] {
  const content = CONTENT_ORDER[area]
  if (looksLikeTaskId(query)) return ['tasks', ...content.filter((section) => section !== 'tasks'), 'navigation']
  return navigationNamed ? ['navigation', ...content] : [...content, 'navigation']
}
