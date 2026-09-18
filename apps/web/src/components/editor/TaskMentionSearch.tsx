import { useEffect, useMemo, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { MentionList } from './MentionList'
import { buildMentionItems, createTaskLookup } from './extensions/mentionItems'
import type { MentionItem } from './extensions/mentionItems'

/**
 * The `@` menu's search, without the editor. Same debounced fts5 query, same
 * `buildMentionItems` caps and ordering, same popover component — just driven by
 * a plain input so it can live in a modal (plan 04's "Mark as duplicate" dialog).
 */
export function TaskMentionSearch({
  workspaceId,
  excludeTaskId,
  onPick,
}: {
  workspaceId: string
  excludeTaskId?: string
  onPick: (taskId: string) => void
}) {
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<MentionItem[]>([])
  const [activeIndex, setActiveIndex] = useState(0)
  // An empty query shows nothing without waiting on (or re-rendering for) a lookup.
  const items = query.trim() === '' ? [] : results
  // One lookup per workspace so the 150ms debounce timer is shared across keystrokes.
  const lookup = useMemo(() => createTaskLookup(workspaceId), [workspaceId])

  useEffect(() => {
    let cancelled = false
    void lookup(query).then((tasks) => {
      // An empty query resolves at once to nothing; there is nothing to store.
      if (cancelled || query.trim() === '') return
      // Exclude BEFORE buildMentionItems so the 8-result cap is not silently shortened.
      const candidates = tasks.filter((task) => task.id !== excludeTaskId)
      setResults(buildMentionItems({ query, members: [], tasks: candidates }))
      setActiveIndex(0)
    })
    return () => {
      cancelled = true
    }
  }, [lookup, query, excludeTaskId])

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      setQuery('')
      setResults([])
      return
    }
    if (items.length === 0) return
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActiveIndex((index) => (index + 1) % items.length)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActiveIndex((index) => (index - 1 + items.length) % items.length)
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      onPick(items[activeIndex].id)
    }
  }

  return (
    <div className="editor-mention-search">
      <input
        type="text"
        className="input"
        autoFocus
        value={query}
        placeholder="Search issues by identifier or title"
        aria-label="Search issues"
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={handleKeyDown}
      />
      <MentionList
        items={items}
        activeIndex={activeIndex}
        label="Search issues"
        onSelect={(item) => onPick(item.id)}
        onHover={setActiveIndex}
      />
    </div>
  )
}
