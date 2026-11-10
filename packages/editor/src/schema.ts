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
import { FileExternalHtml, FilePreview } from './FilePreview'
import { isolateEvents } from './isolation'
import { linkEmbedExtension } from './linkEmbed'

/**
 * Video and audio stay out of the schema. A file block shows the player (and a text or PDF preview) from the
 * file name, and a paragraph that is only a video link shows the player above the link.
 */
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

// Same file props as BlockNote. Only the view changes, so stored documents and the server schema stay as they are.
const fileBlock = baseBlockSpecs.file
const fileWithPreview = createReactBlockSpec(fileBlock.config, {
  meta: fileBlock.implementation.meta,
  parse: fileBlock.implementation.parse,
  render: FilePreview,
  toExternalHTML: FileExternalHtml,
})()

const paragraphBlock = baseBlockSpecs.paragraph
const paragraphWithEmbed: typeof paragraphBlock = {
  ...paragraphBlock,
  extensions: [...(paragraphBlock.extensions ?? []), linkEmbedExtension()],
}

/** A link to another page, rendered as a card. `pageId` is the target page id; the block has no inline content. */
const pageBlockConfig = {
  type: 'page',
  propSchema: {
    pageId: { default: '' },
  },
  content: 'none',
} as const

/**
 * A database shown inside the page: its rows, views and toolbar. `databaseId` is the database page. `linkId` empty
 * shows the database's own views (an inline database); otherwise it is a `database_links` id whose views belong to
 * this block only (a linked view). The host draws its UI inside an `ISOLATED_ATTR` root.
 */
const databaseBlockConfig = {
  type: 'database',
  propSchema: {
    databaseId: { default: '' },
    linkId: { default: '' },
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

/** A formula inside a sentence. `latex` is the source; the host draws it. */
const mathConfig = {
  type: 'math',
  propSchema: {
    latex: { default: '' },
  },
  content: 'none',
} as const

type PageBlockImplementation = ReactCustomBlockImplementation<typeof pageBlockConfig>
type DatabaseBlockImplementation = ReactCustomBlockImplementation<typeof databaseBlockConfig>
type CalloutImplementation = ReactCustomBlockImplementation<typeof calloutBlockConfig>
type MentionImplementation = ReactInlineContentImplementation<typeof mentionConfig, StyleSchemaFromSpecs<typeof defaultStyleSpecs>>
type MathImplementation = ReactInlineContentImplementation<typeof mathConfig, StyleSchemaFromSpecs<typeof defaultStyleSpecs>>

/** How a host draws the custom blocks. Views never change the schema, so every host edits the same documents. */
export interface EditorViews {
  page: { render: PageBlockImplementation['render']; toExternalHTML?: PageBlockImplementation['toExternalHTML'] }
  database: { render: DatabaseBlockImplementation['render']; toExternalHTML?: DatabaseBlockImplementation['toExternalHTML'] }
  callout: { render: CalloutImplementation['render']; toExternalHTML?: CalloutImplementation['toExternalHTML'] }
  mention: { render: MentionImplementation['render'] }
  math: { render: MathImplementation['render'] }
  codeBlock: CodeBlockOptions
}

export function createEditorSchemas(views: EditorViews) {
  const codeBlock = codeBlockSpec(views.codeBlock)
  const callout = createReactBlockSpec(calloutBlockConfig, views.callout)
  const mention = createReactInlineContentSpec(mentionConfig, views.mention)
  const math = createReactInlineContentSpec(mathConfig, views.math)

  const pageEditorSchema = BlockNoteSchema.create({
    blockSpecs: {
      ...baseBlockSpecs,
      file: fileWithPreview,
      paragraph: paragraphWithEmbed,
      codeBlock,
      page: createReactBlockSpec(pageBlockConfig, views.page)(),
      database: isolateEvents(createReactBlockSpec(databaseBlockConfig, views.database)()),
      callout: callout(),
    },
    inlineContentSpecs: { ...defaultInlineContentSpecs, mention, math },
  })

  /**
   * Task descriptions and comments: the page editor's blocks without `page` and `database` (a link to a docs page is
   * a plain link there). The server's converter shares the page schema; its task sanitizer drops both.
   */
  const taskEditorSchema = BlockNoteSchema.create({
    blockSpecs: {
      ...baseBlockSpecs,
      file: fileWithPreview,
      paragraph: paragraphWithEmbed,
      codeBlock,
      callout: callout(),
    },
    inlineContentSpecs: { ...defaultInlineContentSpecs, mention, math },
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
