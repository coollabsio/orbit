/**
 * Client-side mirror of `crates/orbit/src/rich_text.rs`. The server is the
 * authority — it re-derives `description_text` / `body_text` from the stored
 * document on every write — but the editor needs the same answers locally for
 * empty checks and chip resolution.
 */
export interface RichTextNode {
  type: string
  text?: string
  attrs?: Record<string, unknown>
  marks?: { type: string; attrs?: Record<string, unknown> }[]
  content?: RichTextNode[]
}

export interface RichTextDocument extends RichTextNode {
  type: 'doc'
}

export const EMPTY_DOCUMENT: RichTextDocument = { type: 'doc', content: [] }

/** Project keys are uppercase letters, digits and underscores; the suffix is a number. */
export const IDENTIFIER_PATTERN = /\b[A-Z][A-Z0-9_]{0,19}-\d+\b/

// listItem and taskItem are containers: their text lives in a child paragraph,
// so treating them as blocks too would emit every list entry twice.
const BLOCK_TYPES = new Set(['paragraph', 'heading', 'codeBlock'])
const NESTED_IN_BLOCK = new Set(['bulletList', 'orderedList', 'taskList', 'blockquote'])

function isNode(value: unknown): value is RichTextNode {
  return typeof value === 'object' && value !== null && typeof (value as RichTextNode).type === 'string'
}

export function asDocument(value: unknown): RichTextDocument {
  return isNode(value) && value.type === 'doc' ? (value as RichTextDocument) : EMPTY_DOCUMENT
}

function children(node: RichTextNode): RichTextNode[] {
  return Array.isArray(node.content) ? node.content.filter(isNode) : []
}

function inlineText(node: RichTextNode): string {
  let out = ''
  if (node.type === 'text') out = typeof node.text === 'string' ? node.text : ''
  else if (node.type === 'hardBreak') out = '\n'
  else if (node.type === 'mention') out = `@${String(node.attrs?.label ?? '')}`
  else if (node.type === 'taskMention') out = String(node.attrs?.identifier ?? '')
  else if (NESTED_IN_BLOCK.has(node.type)) return ''
  return out + children(node).map(inlineText).join('')
}

function collectBlocks(node: RichTextNode, blocks: string[]): void {
  const isBlock = BLOCK_TYPES.has(node.type)
  if (isBlock) {
    const text = inlineText(node)
    if (text) blocks.push(text)
  }
  for (const child of children(node)) {
    if (isBlock && !NESTED_IN_BLOCK.has(child.type)) continue
    collectBlocks(child, blocks)
  }
}

/** Matches `rich_text::extract_text`: blocks joined with newlines, mentions as their label. */
export function documentText(document: unknown): string {
  if (!isNode(document)) return ''
  const blocks: string[] = []
  collectBlocks(document, blocks)
  return blocks.join('\n')
}

/** Mirrors `rich_text::is_empty`: a document whose derived text is blank. */
export function isEmptyDocument(document: unknown): boolean {
  return documentText(document).trim() === ''
}

export function taskIdentifiersInDocument(document: unknown): string[] {
  const found: string[] = []
  const walk = (node: RichTextNode) => {
    if (node.type === 'taskMention') {
      const identifier = String(node.attrs?.identifier ?? '')
      if (identifier && !found.includes(identifier)) found.push(identifier)
    }
    for (const child of children(node)) walk(child)
  }
  if (isNode(document)) walk(document)
  return found
}
