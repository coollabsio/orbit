/**
 * Client-side mirror of `crates/orbit/src/rich_text.rs`. The server is the
 * authority — it re-derives `description_text` / `body_text` from the stored
 * document on every write — but the editor needs the same answers locally for
 * empty checks and chip resolution.
 */
// Type aliases rather than interfaces, so a document is assignable to the
// generated client's `{ [key: string]: unknown }` body fields.
export type RichTextNode = {
  type: string
  text?: string
  attrs?: Record<string, unknown>
  marks?: { type: string; attrs?: Record<string, unknown> }[]
  content?: RichTextNode[]
}

export type RichTextDocument = RichTextNode & { type: 'doc' }

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

const ALLOWED_NODES = new Set([
  'doc', 'paragraph', 'text', 'heading', 'bulletList', 'orderedList', 'listItem', 'taskList', 'taskItem',
  'blockquote', 'codeBlock', 'horizontalRule', 'hardBreak', 'mention', 'taskMention',
])
const ALLOWED_MARKS = new Set(['bold', 'italic', 'strike', 'code', 'underline', 'link'])

const isHttpUrl = (value: unknown): value is string =>
  typeof value === 'string' && /^https?:\/\//.test(value) && value.length <= 2_000
const nonEmptyLabel = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= 200

/** The allowlisted attrs of one node, or `null` when a required one is unusable (the node is dropped). */
function allowedAttrs(node: RichTextNode): Record<string, unknown> | undefined | null {
  const attrs = node.attrs ?? {}
  switch (node.type) {
    case 'heading': {
      const level = Number(attrs.level)
      return { level: Number.isInteger(level) ? Math.min(Math.max(level, 1), 3) : 1 }
    }
    case 'orderedList': {
      const start = attrs.start
      return typeof start === 'number' && Number.isInteger(start) && start > 1 ? { start } : undefined
    }
    case 'codeBlock':
      return typeof attrs.language === 'string' && attrs.language.length > 0 && attrs.language.length <= 40
        ? { language: attrs.language }
        : undefined
    case 'taskItem':
      return { checked: attrs.checked === true }
    case 'mention':
      return typeof attrs.id === 'string' && nonEmptyLabel(attrs.label) ? { id: attrs.id, label: attrs.label } : null
    case 'taskMention':
      return typeof attrs.id === 'string' && nonEmptyLabel(attrs.identifier)
        ? { id: attrs.id, identifier: attrs.identifier }
        : null
    default:
      return undefined
  }
}

function allowedMarks(node: RichTextNode): RichTextNode['marks'] {
  const marks: NonNullable<RichTextNode['marks']> = []
  for (const mark of node.marks ?? []) {
    if (!mark || !ALLOWED_MARKS.has(mark.type)) continue
    if (mark.type !== 'link') {
      marks.push({ type: mark.type })
      continue
    }
    const href = mark.attrs?.href
    // A link the server would reject keeps its text and loses the link.
    if (!isHttpUrl(href)) continue
    // Only the href is stored: every renderer opens links in a new tab with
    // `noopener noreferrer`, and TipTap's per-mark target/rel defaults would
    // otherwise make an untouched document look edited.
    marks.push({ type: 'link', attrs: { href } })
  }
  return marks.length > 0 ? marks : undefined
}

function sanitizeNode(node: RichTextNode): RichTextNode | null {
  if (!isNode(node) || !ALLOWED_NODES.has(node.type) || node.type === 'doc') return null
  if (node.type === 'text') {
    if (typeof node.text !== 'string' || node.text.length === 0) return null
    const marks = allowedMarks(node)
    return marks ? { type: 'text', text: node.text, marks } : { type: 'text', text: node.text }
  }
  const attrs = allowedAttrs(node)
  if (attrs === null) return null
  const content = children(node)
    .map(sanitizeNode)
    .filter((child): child is RichTextNode => child !== null)
  const clean: RichTextNode = { type: node.type }
  if (attrs) clean.attrs = attrs
  if (content.length > 0) clean.content = content
  return clean
}

const isBlankParagraph = (node: RichTextNode) => node.type === 'paragraph' && (node.content ?? []).length === 0

/**
 * The document exactly as the server's allowlist accepts it
 * (`crates/orbit/src/rich_text.rs::validate`). TipTap serialises attributes the
 * server rejects with a 422 — `orderedList.type`, the link mark's `class` and
 * `title`, the stock mention's `mentionSuggestionChar` — so every document
 * leaves the editor through here. Trailing blank paragraphs (the editor's
 * trailing node) are dropped so an untouched document compares equal.
 */
export function toServerDocument(document: unknown): RichTextDocument {
  const content = children(asDocument(document))
    .map(sanitizeNode)
    .filter((child): child is RichTextNode => child !== null)
  while (content.length > 0 && isBlankParagraph(content[content.length - 1])) content.pop()
  return { type: 'doc', content }
}

/** Structural equality of two documents as the server would store them. */
export function sameDocument(a: unknown, b: unknown): boolean {
  return JSON.stringify(toServerDocument(a)) === JSON.stringify(toServerDocument(b))
}

/** What an editor instance can load: ProseMirror needs at least one block. */
export function editableDocument(document: unknown): RichTextDocument {
  const clean = toServerDocument(document)
  return clean.content && clean.content.length > 0 ? clean : { type: 'doc', content: [{ type: 'paragraph' }] }
}
