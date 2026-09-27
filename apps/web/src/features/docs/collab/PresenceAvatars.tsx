import { Avatar, AvatarFallback, AvatarGroup, AvatarGroupCount } from '@/components/ui/avatar'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { Collaborator } from './presence'

/** Avatars shown before the rest collapses into "+n". */
export const PRESENCE_MAX = 4

function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]!.toUpperCase())
    .join('')
}

/** Header stack of the other people on the page, in their caret colors. Renders nothing while alone. */
export function PresenceAvatars({ people, max = PRESENCE_MAX }: { people: Collaborator[]; max?: number }) {
  if (people.length === 0) return null
  const shown = people.slice(0, max)
  const rest = people.slice(max)
  const names = people.map((person) => person.name).join(', ')
  return (
    <AvatarGroup role="group" aria-label={`Also here: ${names}`} data-testid="presence" className="shrink-0 -space-x-1.5">
      {shown.map((person) => (
        <Tooltip key={person.id}>
          <TooltipTrigger
            render={
              <Avatar
                size="sm"
                className="size-6 after:border-transparent"
                data-presence-user={person.id}
                aria-label={person.name}
                tabIndex={0}
              />
            }
          >
            <AvatarFallback className="text-[10px] font-semibold text-white" style={{ backgroundColor: person.color }}>
              {initials(person.name) || '?'}
            </AvatarFallback>
          </TooltipTrigger>
          <TooltipContent>{person.name}</TooltipContent>
        </Tooltip>
      ))}
      {rest.length > 0 ? (
        <Tooltip>
          <TooltipTrigger
            render={
              <AvatarGroupCount
                className="size-6 text-[10px] font-semibold"
                aria-label={`${rest.length} more: ${rest.map((person) => person.name).join(', ')}`}
                tabIndex={0}
              />
            }
          >
            +{rest.length}
          </TooltipTrigger>
          <TooltipContent>{rest.map((person) => person.name).join(', ')}</TooltipContent>
        </Tooltip>
      ) : null}
    </AvatarGroup>
  )
}
