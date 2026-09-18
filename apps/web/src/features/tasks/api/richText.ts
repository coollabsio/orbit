/**
 * Bridge between the plain-text UI and the rich text documents the API now stores.
 * The editor tasks replace these with real TipTap documents; until then a body is
 * paragraphs of text, and any mention the user picked from the popover becomes a
 * real `mention` node so the server still derives the right recipients.
 */
export type RichTextDocument = { [key: string]: unknown }

export interface MentionTarget {
  id: string
  label: string
}

export function emptyDocument(): RichTextDocument {
  return { type: 'doc', content: [] }
}

/** Longest label first, so "@Ada Lovelace" wins over "@Ada". */
function inlineContent(line: string, mentions: MentionTarget[]): unknown[] {
  const ordered = [...mentions].sort((a, b) => b.label.length - a.label.length)
  const nodes: unknown[] = []
  let rest = line
  outer: while (rest.length > 0) {
    for (const mention of ordered) {
      const needle = `@${mention.label}`
      const at = rest.indexOf(needle)
      if (at === -1) continue
      if (at > 0) nodes.push({ type: 'text', text: rest.slice(0, at) })
      nodes.push({ type: 'mention', attrs: { id: mention.id, label: mention.label } })
      rest = rest.slice(at + needle.length)
      continue outer
    }
    nodes.push({ type: 'text', text: rest })
    break
  }
  return nodes
}

/** One paragraph per line, so a multi-line comment keeps its shape. */
export function documentFromText(text: string, mentions: MentionTarget[] = []): RichTextDocument {
  const lines = text.split('\n').filter((line) => line.length > 0)
  if (lines.length === 0) return emptyDocument()
  return {
    type: 'doc',
    content: lines.map((line) => ({ type: 'paragraph', content: inlineContent(line, mentions) })),
  }
}
