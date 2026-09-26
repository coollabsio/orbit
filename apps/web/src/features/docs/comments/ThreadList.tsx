// The Comments panel's thread list (portalled into the panel by the page editor): the page's open or resolved
// threads in document order, threads whose text is gone last. Clicking a card selects its thread, which scrolls the
// editor to the anchor and highlights it; selecting a highlight in the editor scrolls its card into view.
import { useCallback, useEffect, useMemo, useRef } from 'react'
import { CommentsExtension } from '@blocknote/core/comments'
import { useExtension, useExtensionState, useThreads } from '@blocknote/react'
import { ThreadCard } from './ThreadCard'

export function ThreadList({ filter }: { filter: 'open' | 'resolved' }) {
  const comments = useExtension(CommentsExtension)
  const { selectedThreadId, threadPositions } = useExtensionState(CommentsExtension)
  const threads = useThreads()
  const listRef = useRef<HTMLDivElement>(null)

  const items = useMemo(() => {
    const shown = [...threads.values()].filter((thread) => thread.resolved === (filter === 'resolved'))
    const position = (id: string) => threadPositions.get(id)?.from ?? Number.MAX_VALUE
    return shown
      .sort((a, b) => position(a.id) - position(b.id) || a.createdAt.getTime() - b.createdAt.getTime())
      .map((thread) => ({ thread, orphaned: !threadPositions.has(thread.id) }))
  }, [threads, threadPositions, filter])

  const onSelect = useCallback((threadId: string) => comments.selectThread(threadId), [comments])

  useEffect(() => {
    if (!selectedThreadId) return
    const card = listRef.current?.querySelector(`[data-thread-id="${CSS.escape(selectedThreadId)}"]`)
    card?.scrollIntoView?.({ block: 'nearest', behavior: 'smooth' })
  }, [selectedThreadId])

  return (
    <div ref={listRef} className="flex flex-col gap-2" data-thread-list={filter}>
      {items.map(({ thread, orphaned }) => (
        <ThreadCard
          key={thread.id}
          thread={thread}
          orphaned={orphaned}
          selected={thread.id === selectedThreadId}
          variant="panel"
          onSelect={onSelect}
        />
      ))}
    </div>
  )
}
