/** The gallery and project Fields screens' rules (custom task fields, issue #87), as the web's `TaskFieldForm`. */
import { hasOptions } from '@orbit/api/taskFields'

import type { TaskFieldBody, TaskFieldKind, TaskFieldOptionInput } from './task-api'
import { LABEL_PALETTE } from './tasks'

/** The server keeps at most this many options per field. */
export const OPTION_LIMIT = 100

/** Why the options cannot be saved: an option without a name, or two with the same name (any case); null when they can. */
export function optionsProblem(options: readonly Pick<TaskFieldOptionInput, 'name'>[]): string | null {
  const names = options.map((option) => option.name.trim().toLowerCase())
  const valid = names.every((name, index) => name && names.indexOf(name) === index)
  return valid ? null : 'Each option needs a name, and no two options can have the same name.'
}

/** The next option's colour: the label palette in turn. */
export function nextOptionColor(options: readonly unknown[]): string {
  return LABEL_PALETTE[options.length % LABEL_PALETTE.length]!.color
}

/** The create body of the form: trimmed names, and options only for a select kind. */
export function fieldBody(name: string, kind: TaskFieldKind, options: readonly TaskFieldOptionInput[]): TaskFieldBody {
  return { name: name.trim(), kind, options: hasOptions(kind) ? options.map((option) => ({ ...option, name: option.name.trim() })) : [] }
}

/** `ids` with the one at `index` moved one place up (-1) or down (1); unchanged at an end. */
export function moveId(ids: readonly string[], index: number, by: -1 | 1): string[] {
  const next = [...ids]
  const to = index + by
  if (index < 0 || index >= next.length || to < 0 || to >= next.length) return next
  ;[next[index], next[to]] = [next[to]!, next[index]!]
  return next
}

export function projectsText(count: number): string {
  return count === 0 ? 'Not in a project' : count === 1 ? 'In 1 project' : `In ${count} projects`
}
