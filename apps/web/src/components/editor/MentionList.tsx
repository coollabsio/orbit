import { Avatar } from '../ui/Avatar'
import type { MentionItem } from './extensions/mentionItems'

/** The suggestion popover. Keyboard handling lives in the caller; this is presentation. */
export function MentionList({
  items,
  activeIndex,
  onSelect,
  onHover,
  label = 'Mention a person or issue',
}: {
  items: MentionItem[]
  activeIndex: number
  onSelect: (item: MentionItem) => void
  onHover: (index: number) => void
  /** Overridden by `TaskMentionSearch`, which is a task picker rather than an `@` menu. */
  label?: string
}) {
  if (items.length === 0) return null
  const people = items.filter((item) => item.kind === 'user')
  const issues = items.filter((item) => item.kind === 'task')

  const option = (item: MentionItem, index: number) => (
    <div
      key={`${item.kind}-${item.id}`}
      id={`editor-mention-option-${index}`}
      role="option"
      aria-selected={index === activeIndex}
      className="editor-mention-option"
      data-active={index === activeIndex ? 'true' : undefined}
      onMouseDown={(event) => {
        // Keep focus (and the caret) in the editor or the search input.
        event.preventDefault()
        onSelect(item)
      }}
      onMouseEnter={() => onHover(index)}
    >
      {item.kind === 'user' ? (
        <>
          <Avatar user={undefined} name={item.label} size={18} />
          <span className="editor-mention-label truncate">{item.label}</span>
          <span className="editor-mention-meta">@{item.handle}</span>
        </>
      ) : (
        <>
          <span className="editor-mention-id">{item.identifier}</span>
          <span className="editor-mention-label truncate">{item.title}</span>
        </>
      )}
    </div>
  )

  return (
    <div className="editor-mention-popover" role="listbox" aria-label={label}>
      {people.length > 0 ? <div className="editor-mention-group" role="presentation">People</div> : null}
      {people.map((item) => option(item, items.indexOf(item)))}
      {issues.length > 0 ? <div className="editor-mention-group" role="presentation">Issues</div> : null}
      {issues.map((item) => option(item, items.indexOf(item)))}
    </div>
  )
}
