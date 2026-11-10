// The editors' "#" menu (page body, task description and comments, page comments): tasks found by title or ID, the
// recently updated ones before anything is typed. A pick inserts a link to the task that reads "ENG-12 Title".
import type { Transaction } from '@tiptap/pm/state'
import type { DefaultReactSuggestionItem } from '@blocknote/react'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import type { TaskReference, TaskReferenceSearch } from '@/features/tasks/taskReferences'

export const TASK_TRIGGER = '#'
/** Typing pauses this long before the task search runs, so each key does not cost a request. */
const SEARCH_PAUSE_MS = 150

/** What inserting a task link needs of an editor (the page and the comment editor both fit). */
interface LinkInserter {
  insertInlineContent: (content: [{ type: 'link'; href: string; content: string }, string]) => void
}

/** Inserts a link to the task ("ENG-12 Title") and a space after it at the cursor. */
export function insertTaskLink(editor: LinkInserter, task: TaskReference) {
  editor.insertInlineContent([{ type: 'link', href: `${window.location.origin}${task.path}`, content: `${task.identifier} ${task.title}` }, ' '])
}

export function taskSuggestionItems(tasks: readonly TaskReference[], group: string, insert: (task: TaskReference) => void): DefaultReactSuggestionItem[] {
  return tasks.map((task) => ({
    title: task.title,
    subtext: task.projectName ? `${task.identifier} · ${task.projectName}` : task.identifier,
    icon: <TaskStatusIcon status={task.status} size={16} />,
    group,
    onItemClick: () => insert(task),
  }))
}

/**
 * "#" opens the menu after a space, inside a line and outside code. Not at the start of a block: there "# " makes a
 * heading, as in Markdown.
 */
export function taskTriggerAllowed(tr: Transaction): boolean {
  const { $from } = tr.selection
  if ($from.parent.type.spec.code || $from.parentOffset === 0) return false
  return /\s/.test($from.parent.textBetween($from.parentOffset - 1, $from.parentOffset, undefined, '￼'))
}

/**
 * The menu's entries for `query`: matching tasks, or the recently updated ones for an empty query. A query that
 * starts with a space is no task. `isCurrent` says whether the query is still the latest after the typing pause.
 */
export async function taskMenuItems(
  query: string,
  search: TaskReferenceSearch | null,
  insert: (task: TaskReference) => void,
  isCurrent: () => boolean,
): Promise<DefaultReactSuggestionItem[]> {
  if (!search || /^\s/.test(query)) return []
  const text = query.trim()
  if (text) {
    await new Promise((resolve) => setTimeout(resolve, SEARCH_PAUSE_MS))
    if (!isCurrent()) return []
  }
  const tasks = await search(text).catch(() => [])
  return taskSuggestionItems(tasks, text ? 'Tasks' : 'Recently updated tasks', insert)
}

/** Remembers the latest query of a menu, for `taskMenuItems`'s `isCurrent`. */
export function latestQuery() {
  let latest = ''
  return { set: (query: string) => void (latest = query), is: (query: string) => () => latest === query }
}
