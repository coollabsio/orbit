// Mention logic as a reusable hook (chat composer + docs blocks):
// "@" opens user suggestions; "#" opens channel suggestions and tasks (by title or ID; recently updated ones before
// anything is typed) (substring match, cap 10), ↑/↓ wrap, Enter/Tab insert "@Name " / "#channel " / "ENG-12 ",
// Escape closes.
// suppressRef stops the just-inserted label from reopening the popup.
import { useCallback, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import type { AvatarPerson } from '@/components/common/UserAvatar'
import { looksLikeTaskId, useTaskReferences, type TaskReference } from '@/features/tasks/taskReferences'
import { useDebouncedValue } from './useDebouncedValue'
import type { MentionChannel, MentionPerson } from './mentions'

export interface MentionState {
  start: number
  end: number
  query: string
  trigger: '@' | '#'
}

export interface MentionSuggestion {
  id: string
  label: string
  username: string
  color?: string
  /** The member behind a `user` suggestion; absent for "@channel" / "@here". */
  avatar?: AvatarPerson
  /** The task behind a `task` suggestion. */
  task?: TaskReference
  kind: 'user' | 'channel' | 'task'
}

/** With channels and tasks both listed, each gets a share of the popup. */
const SHARED_OTHERS = 4
const SHARED_TASKS = 6

/**
 * The text after "#" that may name a task, or null when it cannot: it starts with a space (`# Heading`) or is long, so
 * plain text typed after a stray "#" stops searching. Empty: nothing typed yet (recently updated tasks).
 */
export function taskSearchText(query: string): string | null {
  if (/^\s/.test(query) || query.trim().split(/\s+/).length > 4) return null
  return query.trim()
}

export function useMentionAutocomplete<Person extends MentionPerson>(
  users: Person[],
  text: string,
  setText: (next: string) => void,
  inputRef: RefObject<HTMLTextAreaElement | null>,
  afterInsert?: () => void,
  /** Label color per user (the highest role color); undefined keeps the default text color. */
  colorOf?: (user: Person) => string | undefined,
  /** When given, "#" suggests these channels. */
  channels?: MentionChannel[],
  /** When given, "#" also finds the tasks of this workspace. */
  taskWorkspaceId?: string,
) {
  const [mentionState, setMentionState] = useState<MentionState | null>(null)
  const [activeIndex, setActiveIndex] = useState(0)
  const suppressRef = useRef(false)
  const taskQuery = mentionState?.trigger === '#' && taskWorkspaceId ? taskSearchText(mentionState.query) : null
  const debouncedTaskQuery = useDebouncedValue(taskQuery, 150)
  const taskResults = useTaskReferences(taskWorkspaceId ?? '', debouncedTaskQuery ?? '', Boolean(taskWorkspaceId) && debouncedTaskQuery !== null)
  // only the hits of the query as typed: Enter must never pick a task found for an earlier query
  const fresh = taskQuery !== null && debouncedTaskQuery === taskQuery && !taskResults.isPlaceholderData
  const tasks: MentionSuggestion[] = fresh
    ? (taskResults.data ?? []).map((task) => ({ id: `task_${task.id}`, label: task.title, username: task.identifier, task, kind: 'task' as const }))
    : []

  const all: MentionSuggestion[] =
    mentionState?.trigger === '#'
      ? (channels ?? []).map((channel) => ({ id: channel.id, label: channel.name, username: '', kind: 'channel' as const }))
      : users.map((user) => ({
          id: user.id,
          label: user.name,
          username: user.handle,
          color: colorOf ? colorOf(user) : undefined,
          avatar: user.color ? { name: user.name, color: user.color, avatarUrl: user.avatarUrl } : undefined,
          kind: 'user' as const,
        }))
  const query = mentionState?.query.trim().toLowerCase() ?? ''
  const matching = (
    query
      ? all.filter((s) => s.label.toLowerCase().includes(query) || s.username.toLowerCase().includes(query))
      : all
  )
  const suggestions = mixSuggestions(matching, tasks, query)
  const open = mentionState !== null && suggestions.length > 0
  // task hits arrive later and can shorten the list under the highlight
  const active = Math.max(0, Math.min(activeIndex, suggestions.length - 1))

  function update(value: string, cursor: number) {
    if (suppressRef.current) {
      suppressRef.current = false
      setMentionState(null)
      return
    }
    const beforeCursor = value.slice(0, cursor)
    // the later of "@" (users) and "#" (channels and tasks, only when provided) wins
    const triggers: Array<'@' | '#'> = (channels && channels.length > 0) || taskWorkspaceId ? ['@', '#'] : ['@']
    const atIndex = Math.max(...triggers.map((t) => beforeCursor.lastIndexOf(t)))
    if (atIndex === -1) {
      setMentionState(null)
      return
    }
    const trigger = beforeCursor[atIndex] as '@' | '#'
    const nextQuery = beforeCursor.slice(atIndex + 1)
    const charBeforeAt = atIndex > 0 ? beforeCursor[atIndex - 1] : ''
    if ((charBeforeAt && !/\s/.test(charBeforeAt)) || nextQuery.includes('\n') || nextQuery.length > 48) {
      setMentionState(null)
      return
    }
    setMentionState({ start: atIndex, end: cursor, query: nextQuery, trigger })
    setActiveIndex(0)
  }

  function insert(suggestion: MentionSuggestion) {
    if (!mentionState) return
    const el = inputRef.current
    // a task goes in as its identifier, which the message shows as the task's chip
    const insertion = suggestion.kind === 'task' ? `${suggestion.username} ` : `${suggestion.kind === 'channel' ? '#' : '@'}${suggestion.label} `
    const next = `${text.slice(0, mentionState.start)}${insertion}${text.slice(mentionState.end)}`
    const cursor = mentionState.start + insertion.length
    suppressRef.current = true
    setText(next)
    setMentionState(null)
    requestAnimationFrame(() => {
      if (!el) return
      el.focus()
      el.selectionStart = cursor
      el.selectionEnd = cursor
      afterInsert?.()
    })
  }

  const close = useCallback(() => setMentionState(null), [])

  /** Returns true when the key was consumed by the popup. */
  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>): boolean {
    // During IME composition the keys belong to the IME.
    if (!open || e.nativeEvent.isComposing) return false
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActiveIndex((active + 1) % suggestions.length)
      return true
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((active - 1 + suggestions.length) % suggestions.length)
      return true
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      insert(suggestions[active])
      return true
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      setMentionState(null)
      return true
    }
    return false
  }

  return {
    open,
    suggestions,
    activeIndex: active,
    setActiveIndex,
    update,
    insert,
    close,
    handleKeyDown,
    /** "#" with nothing typed yet and tasks on offer: the popup can say how to find a task. */
    emptyTaskQuery: Boolean(taskWorkspaceId) && mentionState?.trigger === '#' && query === '',
  }
}

/**
 * Channels (or people) and tasks in one list of at most 10: tasks first when the query looks like an identifier
 * (`ENG-12`, `12`), the others first otherwise. A kind that has fewer hits leaves its room to the other.
 */
export function mixSuggestions(others: MentionSuggestion[], tasks: MentionSuggestion[], query: string): MentionSuggestion[] {
  if (tasks.length === 0) return others.slice(0, 10)
  const othersRoom = Math.max(SHARED_OTHERS, 10 - Math.min(tasks.length, SHARED_TASKS))
  const shownOthers = others.slice(0, othersRoom)
  const shownTasks = tasks.slice(0, 10 - shownOthers.length)
  return looksLikeTaskId(query) ? [...shownTasks, ...shownOthers] : [...shownOthers, ...shownTasks]
}
