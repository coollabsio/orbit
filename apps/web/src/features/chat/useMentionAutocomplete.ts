// the chat reference MessageInput mention logic as a reusable hook (chat composer + docs blocks):
// "@" opens suggestions (substring match on name/handle, cap 10), ↑/↓ wrap, Enter/Tab insert
// "@Display Name ", Escape closes. suppressRef stops the just-inserted "@label" from reopening.
import { useCallback, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import type { User } from '../../mock/types'

export interface MentionState {
  start: number
  end: number
  query: string
}

export interface MentionSuggestion {
  id: string
  label: string
  username: string
  color?: string
}

export function useMentionAutocomplete(
  users: User[],
  text: string,
  setText: (next: string) => void,
  inputRef: RefObject<HTMLTextAreaElement | null>,
  afterInsert?: () => void,
  /** Label color per user (the highest role color); undefined keeps the default text color. */
  colorOf?: (user: User) => string | undefined,
) {
  const [mentionState, setMentionState] = useState<MentionState | null>(null)
  const [activeIndex, setActiveIndex] = useState(0)
  const suppressRef = useRef(false)

  const all: MentionSuggestion[] = users.map((user) => ({
    id: user.id,
    label: user.name,
    username: user.handle,
    color: colorOf ? colorOf(user) : undefined,
  }))
  const query = mentionState?.query.trim().toLowerCase() ?? ''
  const suggestions = (
    query
      ? all.filter((s) => s.label.toLowerCase().includes(query) || s.username.toLowerCase().includes(query))
      : all
  ).slice(0, 10)
  const open = mentionState !== null && suggestions.length > 0

  function update(value: string, cursor: number) {
    if (suppressRef.current) {
      suppressRef.current = false
      setMentionState(null)
      return
    }
    const beforeCursor = value.slice(0, cursor)
    const atIndex = beforeCursor.lastIndexOf('@')
    if (atIndex === -1) {
      setMentionState(null)
      return
    }
    const nextQuery = beforeCursor.slice(atIndex + 1)
    const charBeforeAt = atIndex > 0 ? beforeCursor[atIndex - 1] : ''
    if ((charBeforeAt && !/\s/.test(charBeforeAt)) || nextQuery.includes('\n') || nextQuery.length > 48) {
      setMentionState(null)
      return
    }
    setMentionState({ start: atIndex, end: cursor, query: nextQuery })
    setActiveIndex(0)
  }

  function insert(suggestion: MentionSuggestion) {
    if (!mentionState) return
    const el = inputRef.current
    const insertion = `@${suggestion.label} `
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
    if (!open) return false
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActiveIndex((index) => (index + 1) % suggestions.length)
      return true
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((index) => (index - 1 + suggestions.length) % suggestions.length)
      return true
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      insert(suggestions[activeIndex])
      return true
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      setMentionState(null)
      return true
    }
    return false
  }

  return { open, suggestions, activeIndex, setActiveIndex, update, insert, close, handleKeyDown }
}
