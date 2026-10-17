import { Avatar, AvatarFallback, AvatarGroup, AvatarGroupCount, AvatarImage } from '@/components/ui/avatar'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'
import { inkOn } from './cursor'
import { initials, type Collaborator } from './presence'

/** Avatars shown before the rest collapses into "+n". */
export const PRESENCE_MAX = 4

function PresenceAvatar({ person }: { person: Collaborator }) {
  return (
    <Avatar
      size="sm"
      className="size-6 after:border-transparent motion-safe:animate-in motion-safe:fade-in motion-safe:zoom-in-75 motion-safe:duration-150"
      data-presence-user={person.id}
      data-editing={person.editing || undefined}
      aria-label={person.name}
      // a ring in their cursor color, full while they edit and faint while they only view
      style={{
        boxShadow: `0 0 0 1.5px var(--background), 0 0 0 3px ${person.editing ? person.color : `color-mix(in oklab, ${person.color} 35%, transparent)`}`,
      }}
    >
      {person.avatarUrl ? <AvatarImage src={person.avatarUrl} alt="" /> : null}
      <AvatarFallback className="text-[10px] font-semibold" style={{ backgroundColor: person.color, color: inkOn(person.color) }}>
        {initials(person.name) || '?'}
      </AvatarFallback>
    </Avatar>
  )
}

/**
 * Header stack of the other people on the page, ringed in their caret colors. Hover names them and says whether they
 * edit or view; a click opens their profile, which for someone editing also offers "Go to cursor" (like Google Docs).
 * Renders nothing while alone.
 */
export function PresenceAvatars({
  people,
  max = PRESENCE_MAX,
  onReveal,
}: {
  people: Collaborator[]
  max?: number
  /** Scrolls to the person's caret. */
  onReveal?: (person: Collaborator) => void
}) {
  if (people.length === 0) return null
  const shown = people.slice(0, max)
  const rest = people.slice(max)
  const names = people.map((person) => person.name).join(', ')
  return (
    <AvatarGroup role="group" aria-label={`Also here: ${names}`} data-testid="presence" className="shrink-0 -space-x-1 px-0.5">
      {shown.map((person) => (
        <Tooltip key={person.id}>
          <TooltipTrigger
            render={
              <ProfileTrigger
                userId={person.id}
                name={person.name}
                kind="avatar"
                action={person.editing && onReveal ? { label: 'Go to cursor', run: () => onReveal(person) } : undefined}
              />
            }
          >
            <PresenceAvatar person={person} />
          </TooltipTrigger>
          <TooltipContent>
            <span className="font-medium">{person.name}</span>
            <span className="text-background/60">{person.editing ? 'Editing' : 'Viewing'}</span>
          </TooltipContent>
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
