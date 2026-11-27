// The mail body's BlockNote schema (§9.1 U8): only what reads well in every mail app. Paragraphs, bullet and numbered
// lists, quotes, code blocks, images and links; bold, italic, underline and strike. No headings, colors, tables or
// files: the server sanitizes the HTML (`clean_outgoing_html`) and the message must look the same everywhere.
// Loaded with the editor (lazily): BlockNote is about 1 MB.
import { BlockNoteEditor, BlockNoteSchema, defaultBlockSpecs, defaultInlineContentSpecs, defaultStyleSpecs } from '@blocknote/core'

const { paragraph, bulletListItem, numberedListItem, quote, codeBlock, image } = defaultBlockSpecs
const { bold, italic, underline, strike } = defaultStyleSpecs

export const mailEditorSchema = BlockNoteSchema.create({
  blockSpecs: { paragraph, bulletListItem, numberedListItem, quote, codeBlock, image },
  // text and links
  inlineContentSpecs: defaultInlineContentSpecs,
  styleSpecs: { bold, italic, underline, strike },
})

export type MailEditorInstance = typeof mailEditorSchema.BlockNoteEditor
export type MailBlock = MailEditorInstance['document'][number]

let parser: MailEditorInstance | null = null

// BlockNote writes an empty block as `<p>\u{FFFC}</p>` so that it can parse it back. Mail apps show that character as a
// box, so the mail HTML has `<p><br></p>` (a blank line) in its place, and the parser gets the placeholder back.
const EMPTY_BLOCK_PLACEHOLDER = '\uFFFC'
const EMPTY_PARAGRAPH = /<p><br\s*\/?><\/p>/g

/** The mail HTML of blocks: empty lines are `<p><br></p>`. */
export function mailBlocksToHtml(editor: MailEditorInstance, blocks: MailBlock[]): string {
  return editor.blocksToHTMLLossy(blocks).replaceAll(EMPTY_BLOCK_PLACEHOLDER, '<br>')
}

/** Blocks of stored HTML (a draft body, a signature, a snippet), without an editor on screen. */
export function htmlToMailBlocks(html: string): MailBlock[] {
  if (html.trim() === '') return []
  parser ??= BlockNoteEditor.create({ schema: mailEditorSchema, _tiptapOptions: { injectCSS: false } }) as unknown as MailEditorInstance
  return parser.tryParseHTMLToBlocks(html.replace(EMPTY_PARAGRAPH, `<p>${EMPTY_BLOCK_PLACEHOLDER}</p>`))
}

type InlineItem = { type: string; text?: string; content?: unknown }

/** The plain text of one block's own line (not its children). */
export function blockText(block: { content?: unknown }): string {
  const content = block.content
  if (!Array.isArray(content)) return ''
  return (content as InlineItem[])
    .map((item) => (item.type === 'text' ? (item.text ?? '') : item.type === 'link' && Array.isArray(item.content) ? blockText(item) : ''))
    .join('')
}

/** Every line of the document, children after their parent; without code blocks with `skipCode`. */
export function mailText(blocks: readonly MailBlock[], { skipCode = false } = {}): string {
  const lines: string[] = []
  const walk = (list: readonly MailBlock[]) => {
    for (const block of list) {
      if (!(skipCode && block.type === 'codeBlock')) lines.push(blockText(block))
      walk(block.children as MailBlock[])
    }
  }
  walk(blocks)
  return lines.join('\n')
}

/** A document with nothing in it: empty text blocks only. */
export const isBlankDocument = (blocks: readonly MailBlock[]): boolean =>
  blocks.every((block) => block.type !== 'image' && blockText(block).trim() === '' && isBlankDocument(block.children as MailBlock[]))
