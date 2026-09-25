import { BlockNoteSchema, defaultBlockSpecs, type BlockNoteEditor } from '@blocknote/core'
import { createReactBlockSpec } from '@blocknote/react'
import { CALLOUT_DEFAULT_BACKGROUND, CALLOUT_DEFAULT_EMOJI, CalloutExternalView, CalloutView } from './CalloutBlock'
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
    page: pageBlockSpec(),
    callout: calloutBlockSpec(),
  },
})

export type PageEditorSchema = typeof pageEditorSchema
export type PageEditorInstance = BlockNoteEditor<
  PageEditorSchema['blockSchema'],
  PageEditorSchema['inlineContentSchema'],
  PageEditorSchema['styleSchema']
>

/** Every block type the editor accepts; stored content with other types is dropped on load. */
export const EDITOR_BLOCK_TYPES: ReadonlySet<string> = new Set(Object.keys(pageEditorSchema.blockSchema))
