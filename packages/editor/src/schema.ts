// The BlockNote schema of Orbit's page and task documents. Web and mobile build their editors from
// `createEditorSchemas`: the block configs here decide the ProseMirror schema both bind to the same Y.Doc, and the
// server's converter reads it (`apps/server/src/collab/blocknote-schema.json`). Hosts only choose how blocks look.
import { BlockNoteSchema, defaultBlockSpecs, defaultInlineContentSpecs, defaultStyleSpecs, type BlockNoteEditor, type StyleSchemaFromSpecs } from '@blocknote/core'
import {
  createReactBlockSpec,
  createReactInlineContentSpec,
  type ReactCustomBlockImplementation,
  type ReactInlineContentImplementation,
} from '@blocknote/react'
import { codeBlockSpec, type CodeBlockOptions } from './codeHighlight'

/** Media blocks without page-file support: video and audio stay out (images and files upload as page files). */
export const REMOVED_BLOCK_TYPES = ['video', 'audio'] as const

/** Blocks whose `props.url` points at an uploaded page file (or an external http(s) URL). */
export const FILE_BLOCK_TYPES = ['image', 'file'] as const
type RemovedBlockType = (typeof REMOVED_BLOCK_TYPES)[number]

/** Emoji of a new callout (and of imported Notion callouts whose icon is not an emoji). */
export const CALLOUT_DEFAULT_EMOJI = '💡'
/** Background of a new callout; any BlockNote color name, `default` = no fill, just a border. */
export const CALLOUT_DEFAULT_BACKGROUND = 'gray'

const baseBlockSpecs = Object.fromEntries(
  Object.entries(defaultBlockSpecs).filter(([type]) => !(REMOVED_BLOCK_TYPES as readonly string[]).includes(type)),
) as Omit<typeof defaultBlockSpecs, RemovedBlockType>

/** A link to another page, rendered as a card. `pageId` is the target page id; the block has no inline content. */
const pageBlockConfig = {
  type: 'page',
  propSchema: {
    pageId: { default: '' },
  },
  content: 'none',
} as const

/**
 * A Notion-style callout: an emoji next to rich text, in a rounded colored box; nested blocks sit inside the box.
 * `backgroundColor`/`textColor` are BlockNote color names.
 */
const calloutBlockConfig = {
  type: 'callout',
  propSchema: {
    emoji: { default: CALLOUT_DEFAULT_EMOJI },
    backgroundColor: { default: CALLOUT_DEFAULT_BACKGROUND },
    textColor: { default: 'default' },
  },
  content: 'inline',
} as const

/** An @mention of a member ({ userId, name }); the server notifies newly mentioned members. */
const mentionConfig = {
  type: 'mention',
  propSchema: {
    userId: { default: '' },
    name: { default: '' },
  },
  content: 'none',
} as const

type PageBlockImplementation = ReactCustomBlockImplementation<typeof pageBlockConfig>
type CalloutImplementation = ReactCustomBlockImplementation<typeof calloutBlockConfig>
type MentionImplementation = ReactInlineContentImplementation<typeof mentionConfig, StyleSchemaFromSpecs<typeof defaultStyleSpecs>>

/** How a host draws the custom blocks. Views never change the schema, so every host edits the same documents. */
export interface EditorViews {
  page: { render: PageBlockImplementation['render']; toExternalHTML?: PageBlockImplementation['toExternalHTML'] }
  callout: { render: CalloutImplementation['render']; toExternalHTML?: CalloutImplementation['toExternalHTML'] }
  mention: { render: MentionImplementation['render'] }
  codeBlock: CodeBlockOptions
}

export function createEditorSchemas(views: EditorViews) {
  const codeBlock = codeBlockSpec(views.codeBlock)
  const callout = createReactBlockSpec(calloutBlockConfig, views.callout)
  const mention = createReactInlineContentSpec(mentionConfig, views.mention)

  const pageEditorSchema = BlockNoteSchema.create({
    blockSpecs: {
      ...baseBlockSpecs,
      codeBlock,
      page: createReactBlockSpec(pageBlockConfig, views.page)(),
      callout: callout(),
    },
    inlineContentSpecs: { ...defaultInlineContentSpecs, mention },
  })

  /**
   * Task descriptions and comments: the page editor's blocks without `page` (a link to a docs page is a plain link
   * there). The server's converter shares the page schema; its task sanitizer drops `page` blocks.
   */
  const taskEditorSchema = BlockNoteSchema.create({
    blockSpecs: {
      ...baseBlockSpecs,
      codeBlock,
      callout: callout(),
    },
    inlineContentSpecs: { ...defaultInlineContentSpecs, mention },
  })

  return { pageEditorSchema, taskEditorSchema }
}

type EditorSchemas = ReturnType<typeof createEditorSchemas>
export type PageEditorSchema = EditorSchemas['pageEditorSchema']
export type TaskEditorSchema = EditorSchemas['taskEditorSchema']
export type PageEditorInstance = BlockNoteEditor<
  PageEditorSchema['blockSchema'],
  PageEditorSchema['inlineContentSchema'],
  PageEditorSchema['styleSchema']
>
