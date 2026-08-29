import { User } from 'reicon-react'
import type { MentionSuggestion } from '../useMentionAutocomplete'

/** the chat reference mention popup: avatar, @label, handle; onMouseDown keeps the textarea focused. */
export function MentionPopover({
  suggestions,
  activeIndex,
  onSelect,
  onHover,
  placement = 'above',
}: {
  suggestions: MentionSuggestion[]
  activeIndex: number
  onSelect: (suggestion: MentionSuggestion) => void
  onHover: (index: number) => void
  placement?: 'above' | 'below'
}) {
  if (suggestions.length === 0) return null
  return (
    <div className="fc-mention-popover" data-placement={placement}>
      <div className="fc-popover-label">Mentions</div>
      {suggestions.map((suggestion, index) => (
        <button
          key={suggestion.id}
          type="button"
          className="fc-mention-option"
          data-active={index === activeIndex ? 'true' : undefined}
          onMouseDown={(event) => {
            event.preventDefault()
            onSelect(suggestion)
          }}
          onMouseEnter={() => onHover(index)}
        >
          <span className="fc-mention-avatar">
            {suggestion.label.charAt(0).toUpperCase()}
          </span>
          <span className="fc-mention-label" style={suggestion.color ? { color: suggestion.color } : undefined}>
            @{suggestion.label}
          </span>
          <span className="fc-mention-username">
            <User size={12} />
            {suggestion.username}
          </span>
        </button>
      ))}
    </div>
  )
}
