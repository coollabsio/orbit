// The page body's @mention: a `mention` inline node ({ userId, name }) rendered as a non-editable chip. `name` is the
// member's name when they were mentioned (export and fallback); the chip shows the current name when it is known.
import { createReactInlineContentSpec } from '@blocknote/react'
import { PageMentionChip } from './PageMentionChip'

export const pageMentionSpec = createReactInlineContentSpec(
  {
    type: 'mention',
    propSchema: {
      userId: { default: '' },
      name: { default: '' },
    },
    content: 'none',
  },
  {
    render: ({ inlineContent }) => <PageMentionChip userId={inlineContent.props.userId} name={inlineContent.props.name} />,
  },
)
