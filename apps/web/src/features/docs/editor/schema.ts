import { BlockNoteEditor as BlockNoteEditorClass, BlockNoteSchema, defaultBlockSpecs, defaultInlineContentSpecs, type BlockNoteEditor } from '@blocknote/core'
import { createReactBlockSpec } from '@blocknote/react'
import { codeBlockSpec } from './codeBlock'
import { CALLOUT_DEFAULT_BACKGROUND, CALLOUT_DEFAULT_EMOJI, CalloutExternalView, CalloutView } from './CalloutBlock'
import { pageMentionSpec } from './MentionInline'
import { PageBlockExternalView, PageBlockView } from './PageBlockCard'

/** Media blocks without page-file support: video and audio stay out (images and files upload as page files). */
export const REMOVED_BLOCK_TYPES = ['video', 'audio'] as const

/** Blocks whose `props.url` points at an uploaded page file (or an external http(s) URL). */
export const FILE_BLOCK_TYPES = ['image', 'file'] as const
type RemovedBlockType = (typeof REMOVED_BLOCK_TYPES)[number]

const baseBlockSpecs = Object.fromEntries(
  Object.entries(defaultBlockSpecs).filter(([type]) => !(REMOVED_BLOCK_TYPES as readonly string[]).includes(type)),
) as Omit<typeof defaultBlockSpecs, RemovedBlockType>

/** A link to another page, rendered as a card. `pageId` is the target page id; the block has no inline content. */
export const pageBlockSpec = createReactBlockSpec(
  {
    type: 'page',
    propSchema: {
      pageId: { default: '' },
    },
    content: 'none',
  },
  {
    render: PageBlockView,
    toExternalHTML: PageBlockExternalView,
  },
)

/**
 * A Notion-style callout: an emoji (click to change) next to rich text, in a rounded colored box; nested blocks
 * sit inside the box. `backgroundColor`/`textColor` are BlockNote color names (the block "Colors" menu edits them).
 */
export const calloutBlockSpec = createReactBlockSpec(
  {
    type: 'callout',
    propSchema: {
      emoji: { default: CALLOUT_DEFAULT_EMOJI },
      backgroundColor: { default: CALLOUT_DEFAULT_BACKGROUND },
      textColor: { default: 'default' },
    },
    content: 'inline',
  },
  {
    render: CalloutView,
    toExternalHTML: CalloutExternalView,
  },
)

export const pageEditorSchema = BlockNoteSchema.create({
  blockSpecs: {
    ...baseBlockSpecs,
    codeBlock: codeBlockSpec,
    page: pageBlockSpec(),
    callout: calloutBlockSpec(),
  },
  // `mention`: an @mention of a member ({ userId, name }); the server notifies newly mentioned members.
  inlineContentSpecs: { ...defaultInlineContentSpecs, mention: pageMentionSpec },
})

/**
 * Task descriptions and comments: the page editor's blocks without `page` (a link to a docs page is a plain link
 * there). The server's converter shares the page schema; its task sanitizer drops `page` blocks.
 */
export const taskEditorSchema = BlockNoteSchema.create({
  blockSpecs: {
    ...baseBlockSpecs,
    codeBlock: codeBlockSpec,
    callout: calloutBlockSpec(),
  },
  inlineContentSpecs: { ...defaultInlineContentSpecs, mention: pageMentionSpec },
})

/** Every block type the task editor accepts. */
export const TASK_EDITOR_BLOCK_TYPES: ReadonlySet<string> = new Set(Object.keys(taskEditorSchema.blockSchema))

let markdownParser: BlockNoteEditor<typeof taskEditorSchema.blockSchema, typeof taskEditorSchema.inlineContentSchema, typeof taskEditorSchema.styleSchema> | null = null

/** Blocks for task text that exists only as markdown (written before rich text); `@Name` stays text. */
export function markdownToTaskBlocks(markdown: string): unknown[] {
  markdownParser ??= BlockNoteEditorClass.create({ schema: taskEditorSchema, _tiptapOptions: { injectCSS: false } })
  return markdownParser.tryParseMarkdownToBlocks(markdown)
}

export type PageEditorSchema = typeof pageEditorSchema
export type PageEditorInstance = BlockNoteEditor<
  PageEditorSchema['blockSchema'],
  PageEditorSchema['inlineContentSchema'],
  PageEditorSchema['styleSchema']
>

/** Every block type the editor accepts; stored content with other types is dropped on load. */
export const EDITOR_BLOCK_TYPES: ReadonlySet<string> = new Set(Object.keys(pageEditorSchema.blockSchema))
