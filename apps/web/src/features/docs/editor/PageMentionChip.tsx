// The page body's @mention chip.
import { use } from 'react'
import { mentionLabel, PageMentionNamesContext } from './pageMentions'

/** "@Name", not editable inside; the current name when known, else the stored one, "Unknown user" once gone. */
export function PageMentionChip({ userId, name }: { userId: string; name: string }) {
  const names = use(PageMentionNamesContext)
  const label = mentionLabel(userId, name, names)
  return (
    <span
      data-slot="page-mention"
      data-mention-user={userId}
      className="rounded-[4px] bg-primary/10 px-0.5 font-medium whitespace-nowrap text-primary"
      contentEditable={false}
    >
      @{label}
    </span>
  )
}

/** `render` of the `mention` inline content: `name` is the member's name when they were mentioned. */
export function PageMentionView({ inlineContent }: { inlineContent: { props: { userId: string; name: string } } }) {
  return <PageMentionChip userId={inlineContent.props.userId} name={inlineContent.props.name} />
}
