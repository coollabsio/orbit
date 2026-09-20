// the chat reference MessageInput mention logic as a reusable hook (chat composer + docs blocks):
// "@" opens user suggestions, "#" opens channel suggestions (substring match, cap 10), ↑/↓ wrap,
// Enter/Tab insert "@Name " / "#channel ", Escape closes. suppressRef stops the just-inserted
// label from reopening the popup.
import { useCallback, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import type { Channel, User } from '@/mock/types'

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
  kind: 'user' | 'channel'
}

export function useMentionAutocomplete(
  users: User[],
  text: string,
  setText: (next: string) => void,
  inputRef: RefObject<HTMLTextAreaElement | null>,
  afterInsert?: () => void,
  /** Label color per user (the highest role color); undefined keeps the default text color. */
  colorOf?: (user: User) => string | undefined,
  /** When given, "#" suggests these channels. */
  channels?: Channel[],
) {
  const [mentionState, setMentionState] = useState<MentionState | null>(null)
  const [activeIndex, setActiveIndex] = useState(0)
  const suppressRef = useRef(false)

  const all: MentionSuggestion[] =
    mentionState?.trigger === '#'
      ? (channels ?? []).map((channel) => ({ id: channel.id, label: channel.name, username: '', kind: 'channel' as const }))
      : users.map((user) => ({
          id: user.id,
          label: user.name,
          username: user.handle,
          color: colorOf ? colorOf(user) : undefined,
          kind: 'user' as const,
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
    // the later of "@" (users) and "#" (channels, only when provided) wins
    const triggers: Array<'@' | '#'> = channels && channels.length > 0 ? ['@', '#'] : ['@']
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
    const insertion = `${suggestion.kind === 'channel' ? '#' : '@'}${suggestion.label} `
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
