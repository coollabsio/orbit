import { useState } from 'react'
import { Emoji } from '@/components/common/Emoji'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

export interface ReactionChip {
  /** A Unicode emoji, or `:name:` for a custom emoji of the workspace. */
  emoji: string
  /** Who reacted, the first person first. */
  userIds: readonly string[]
}

/** "Ada, Grace and you". */
function reactorNames(userIds: readonly string[], nameOf: (id: string) => string, currentUserId: string | null): string {
  const names = userIds.filter((id) => id !== currentUserId).map(nameOf)
  if (currentUserId && userIds.includes(currentUserId)) names.push(names.length > 0 ? 'you' : 'You')
  if (names.length <= 1) return names.join('')
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}

/**
 * Reaction chips under a chat message or a task comment. The user's own reaction is tinted pink; the tooltip names
 * who reacted; a click adds or removes the user's reaction.
 */
export function ReactionChips({ reactions, currentUserId, nameOf, onToggle, slot = 'reactions' }: {
  reactions: readonly ReactionChip[]
  currentUserId: string | null
  nameOf: (userId: string) => string
  onToggle: (emoji: string) => void
  /** The `data-slot` of the row. */
  slot?: string
}) {
  // Chips that were there when the row appeared do not animate; one added later scales in.
  const [initial] = useState(() => new Set(reactions.map((reaction) => reaction.emoji)))
  if (reactions.length === 0) return null

  return (
    <div data-slot={slot} className="mt-1 flex flex-wrap gap-1">
      {reactions.map((reaction) => {
        const own = currentUserId !== null && reaction.userIds.includes(currentUserId)
        const names = reactorNames(reaction.userIds, nameOf, currentUserId)
        return (
          <Tooltip key={reaction.emoji}>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  data-slot="reaction"
                  data-own={own ? '' : undefined}
                  data-new={initial.has(reaction.emoji) ? undefined : ''}
                  aria-pressed={own}
                  aria-label={`${reaction.emoji} ${reaction.userIds.length}, ${names} reacted`}
                  className="inline-flex h-6 min-w-10 items-center justify-center gap-1 rounded-full border border-border bg-muted/50 px-2 text-xs font-medium text-muted-foreground tabular-nums outline-none transition-[scale] duration-[120ms] ease-out animation-duration-150 focus-visible:ring-3 focus-visible:ring-ring/50 active:scale-[0.97] data-new:animate-in data-new:fade-in-0 data-new:zoom-in-90 data-own:border-primary/40 data-own:bg-primary/10 data-own:text-foreground hover-fine:not-data-own:hover:bg-muted"
                  onClick={() => onToggle(reaction.emoji)}
                />
              }
            >
              <Emoji value={reaction.emoji} />
              {reaction.userIds.length}
            </TooltipTrigger>
            <TooltipContent>
              {names} reacted with {reaction.emoji}
            </TooltipContent>
          </Tooltip>
        )
      })}
    </div>
  )
}
