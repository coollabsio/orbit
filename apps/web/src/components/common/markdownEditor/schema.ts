import { BlockNoteSchema, defaultBlockSpecs, defaultInlineContentSpecs, type BlockNoteEditor } from '@blocknote/core'
import { codeBlockSpec } from '@/features/docs/editor/codeBlock'
import { pageMentionSpec } from '@/features/docs/editor/MentionInline'

const { paragraph, heading, bulletListItem, numberedListItem, checkListItem, quote, table, image, divider } = defaultBlockSpecs

/**
 * The blocks of a task description or comment. The stored format is markdown, so only blocks that survive
 * markdown → blocks → markdown are here (`roundTrip.test.ts`). Mermaid is a code block with the language `mermaid`.
 */
export const markdownEditorSchema = BlockNoteSchema.create({
  blockSpecs: { paragraph, heading, bulletListItem, numberedListItem, checkListItem, quote, codeBlock: codeBlockSpec, table, image, divider },
  // `mention`: a member chip ({ userId, name }); the markdown has `@Name`, as the renderer reads it
  inlineContentSpecs: { ...defaultInlineContentSpecs, mention: pageMentionSpec },
})

export type MarkdownEditorSchema = typeof markdownEditorSchema
export type MarkdownEditorInstance = BlockNoteEditor<
  MarkdownEditorSchema['blockSchema'],
  MarkdownEditorSchema['inlineContentSchema'],
  MarkdownEditorSchema['styleSchema']
>
