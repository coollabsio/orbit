import { use } from 'react'
import { MentionNamesContext } from './mentionContext'

export function MentionChip({ userId, name }: { userId: string; name: string }) {
  const names = use(MentionNamesContext)
  return (
    <span className="orbit-mention" data-mention-user={userId}>
      @{names.get(userId) ?? name}
    </span>
  )
}
