import { Emoji } from '@/components/common/Emoji'
import { Button } from '@/components/ui/button'
import type { EmojiShortcode } from '../../lib/emojiSearch'

/**
 * The list above the message box for `:name`. `onMouseDown` keeps the focus in the text field, which names this list
 * (`aria-controls`) and its active option (`aria-activedescendant`, `${id}-${index}`) for a screen reader.
 */
export function EmojiSuggestions({
  id,
  query,
  suggestions,
  activeIndex,
  onSelect,
  onHover,
}: {
  id: string
  query: string
  suggestions: EmojiShortcode[]
  activeIndex: number
  onSelect: (suggestion: EmojiShortcode) => void
  onHover: (index: number) => void
}) {
  return (
    <div
      data-slot="emoji-suggestions"
      className="absolute right-0 bottom-full left-0 z-50 mb-3 max-h-80 overflow-y-auto overscroll-contain rounded-lg border border-border bg-popover py-2 text-popover-foreground shadow-xl"
    >
      <div aria-hidden="true" className="px-3 pb-1 text-xs font-medium text-muted-foreground">Emoji matching :{query}</div>
      <div id={id} role="listbox" aria-label={`Emoji matching ${query}`}>
      {suggestions.map((suggestion, index) => (
        <Button
          key={suggestion.emoji}
          id={`${id}-${index}`}
          type="button"
          role="option"
          variant="ghost"
          aria-selected={index === activeIndex}
          data-active={index === activeIndex ? 'true' : undefined}
          className="h-auto w-full justify-start gap-3 rounded-none px-3 py-1.5 text-left font-normal transition-none data-[active=true]:bg-muted"
          onMouseDown={(event) => {
            event.preventDefault()
            onSelect(suggestion)
          }}
          onMouseEnter={() => onHover(index)}
        >
          <span className="text-xl">
            <Emoji value={suggestion.emoji} />
          </span>
          <span className="min-w-0 flex-1 truncate text-sm">:{suggestion.name}:</span>
        </Button>
      ))}
      </div>
    </div>
  )
}
