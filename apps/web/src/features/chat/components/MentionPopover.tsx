import { Hashtag, User } from 'reicon-react'
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
  const channelList = suggestions[0].kind === 'channel'
  return (
    <div className="fc-mention-popover" data-placement={placement}>
      <div className="fc-popover-label">{channelList ? 'Channels' : 'Mentions'}</div>
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
            {suggestion.kind === 'channel' ? <Hashtag size={13} /> : suggestion.label.charAt(0).toUpperCase()}
          </span>
          <span className="fc-mention-label" style={suggestion.color ? { color: suggestion.color } : undefined}>
            {suggestion.kind === 'channel' ? '#' : '@'}
            {suggestion.label}
          </span>
          {suggestion.kind === 'user' ? (
            <span className="fc-mention-username">
              <User size={12} />
              {suggestion.username}
            </span>
          ) : null}
        </button>
      ))}
    </div>
  )
}
