// The page body's @mention chip and the picker's member avatar.
import { use, type CSSProperties } from 'react'
import { mentionLabel, PageMentionNamesContext, type PageMentionMember } from './pageMentions'

/** "@Name", not editable inside; the current name when known, else the stored one, "Unknown user" once gone. */
export function PageMentionChip({ userId, name }: { userId: string; name: string }) {
  const names = use(PageMentionNamesContext)
  const label = mentionLabel(userId, name, names)
  return (
    <span
      className="orbit-page-mention rounded-[4px] bg-primary/10 px-0.5 font-medium whitespace-nowrap text-primary"
      data-mention-user={userId}
      contentEditable={false}
    >
      @{label}
    </span>
  )
}

function initials(name: string): string {
  return (
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase() ?? '')
      .join('') || '?'
  )
}

export function MentionAvatar({ member }: { member: PageMentionMember }) {
  const style: CSSProperties | undefined = member.color
    ? { background: `color-mix(in srgb, ${member.color} 22%, transparent)`, color: member.color }
    : undefined
  return (
    <span
      className="inline-flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-[10px] font-semibold text-muted-foreground"
      style={style}
      aria-hidden
    >
      {initials(member.name)}
    </span>
  )
}
