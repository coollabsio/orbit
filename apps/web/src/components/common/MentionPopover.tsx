import { Hashtag as Hash, User } from 'reicon-react'
import type { MentionSuggestion } from '@/lib/useMentionAutocomplete'
import { Button } from '@/components/ui/button'

/** the chat reference mention popup: avatar, @label, handle; onMouseDown keeps the textarea focused. */
export function MentionPopover({
  suggestions,
  activeIndex,
  onSelect,
  onHover,
  placement = 'above',
  showHandle = true,
}: {
  suggestions: MentionSuggestion[]
  activeIndex: number
  onSelect: (suggestion: MentionSuggestion) => void
  onHover: (index: number) => void
  placement?: 'above' | 'below'
  /** Docs hide the right-side handle column. */
  showHandle?: boolean
}) {
  if (suggestions.length === 0) return null
  const channelList = suggestions[0].kind === 'channel'
  return (
    <div
      className={`absolute z-50 max-h-80 overflow-y-auto border border-border bg-popover py-2 shadow-xl ${
        placement === 'below'
          ? 'top-[calc(100%+6px)] left-[30px] w-[min(340px,calc(100vw-48px))] rounded-[10px]'
          : 'bottom-full right-0 left-0 mb-3 rounded-lg'
      }`}
      data-placement={placement}
    >
      <div className="px-3 pb-1 text-xs font-bold tracking-wider text-muted-foreground uppercase">{channelList ? 'Channels' : 'Mentions'}</div>
      {suggestions.map((suggestion, index) => (
        <Button
          key={suggestion.id}
          type="button"
          variant="ghost"
          className="flex h-auto w-full items-center justify-start gap-3 rounded-none border-0 px-3 py-2 text-left font-normal text-foreground transition-colors hover:bg-muted/70 hover:text-foreground dark:hover:bg-muted/70 data-[active=true]:bg-muted active:not-aria-[haspopup]:translate-y-0"
          data-active={index === activeIndex ? 'true' : undefined}
          onMouseDown={(event) => {
            event.preventDefault()
            onSelect(suggestion)
          }}
          onMouseEnter={() => onHover(index)}
        >
          <span className="grid size-7 shrink-0 place-items-center rounded-full bg-muted text-xs font-bold text-muted-foreground">
            {suggestion.kind === 'channel' ? <Hash className="size-[13px]" /> : suggestion.label.charAt(0).toUpperCase()}
          </span>
          <span className="min-w-0 flex-1 truncate text-sm font-semibold" style={suggestion.color ? { color: suggestion.color } : undefined}>
            {suggestion.kind === 'channel' ? suggestion.label : `@${suggestion.label}`}
          </span>
          {suggestion.kind === 'user' && showHandle ? (
            <span className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
              <User className="size-3" />
              {suggestion.username}
            </span>
          ) : null}
        </Button>
      ))}
    </div>
  )
}
