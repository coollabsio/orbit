import { use } from 'react'
import { MentionNamesContext } from './mentionContext'

/** "@Name" in a comment (comment editors are the only place it renders). */
export function MentionChip({ userId, name }: { userId: string; name: string }) {
  const names = use(MentionNamesContext)
  return (
    <span
      data-slot="mention"
      data-mention-user={userId}
      className="font-medium whitespace-nowrap text-primary dark:text-[color-mix(in_oklab,var(--primary),white_35%)]"
    >
      @{names.get(userId) ?? name}
    </span>
  )
}
