/**
 * Bridge between the plain-text UI and the rich text documents the API now stores.
 * The editor tasks replace these with real TipTap documents; until then a body is a
 * single paragraph and reading a document means reading its derived text column.
 */
export type RichTextDocument = { [key: string]: unknown }

export function emptyDocument(): RichTextDocument {
  return { type: 'doc', content: [] }
}

/** One paragraph per line, so a multi-line comment keeps its shape. */
export function documentFromText(text: string): RichTextDocument {
  const lines = text.split('\n').filter((line) => line.length > 0)
  if (lines.length === 0) return emptyDocument()
  return {
    type: 'doc',
    content: lines.map((line) => ({ type: 'paragraph', content: [{ type: 'text', text: line }] })),
  }
}
