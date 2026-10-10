// Markdown in and out of the task editor. The stored format of a description or a comment is markdown; the editor
// works on BlockNote blocks. BlockNote's markdown conversion is lossy, so two rules keep the text safe:
//  - `roundTrips`: the editor opens only on text that it gives back unchanged (after `normalizeMarkdown`);
//  - the callers save only after the person changed the document.
import { isMentionBoundary, mentionedUserId, type MentionToken } from '@/lib/mentions'
import type { MarkdownEditorInstance } from './schema'

/** Applies `change` to the lines outside fenced code blocks; a code block stays as it is. */
function outsideCode(markdown: string, change: (text: string) => string): string {
  const out: string[] = []
  let pending: string[] = []
  let fence: string | null = null
  const flush = () => {
    if (pending.length > 0) out.push(change(pending.join('\n')))
    pending = []
  }
  for (const line of markdown.split('\n')) {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1]
    if (fence === null && marker) {
      flush()
      fence = marker
      // BlockNote writes "text" for a code block without a language
      out.push(line.replace(/^(\s*(?:`{3,}|~{3,}))text\s*$/, '$1'))
    } else if (fence !== null) {
      out.push(line)
      if (marker && marker[0] === fence[0] && marker.length >= fence.length && line.trim() === marker) fence = null
    } else {
      pending.push(line)
    }
  }
  flush()
  return out.join('\n')
}

const tableRow = (line: string) =>
  `| ${line.trim().replace(/^\||\|$/g, '').split(/(?<!\\)\|/).map((cell) => {
    const text = cell.trim()
    return /^:?-+:?$/.test(text) ? text.replace(/-+/, '---') : text
  }).join(' | ')} |`

/**
 * One spelling for markdown that means the same: `-` bullets, `---` dividers, a plain newline for a line break (the
 * renderer shows each newline), table cells without padding, an attachment image as its path. The editor's
 * output goes through this before it is saved, and `roundTrips` compares through it.
 */
export function normalizeMarkdown(markdown: string, origin = window.location.origin): string {
  const text = outsideCode(markdown.replace(/\r\n?/g, '\n'), (part) =>
    part
      // a hard break: two trailing spaces, or a backslash (BlockNote adds a space on the next line)
      .replace(/\\\n(> ?)? ?/g, '\n$1')
      .split('\n')
      .map((line) => {
        const trimmed = line.replace(/[ \t]+$/, '')
        if (/^\s*\*\*\*\s*$/.test(trimmed)) return '---'
        if (/^\s*\|.*\|$/.test(trimmed)) return tableRow(trimmed)
        return trimmed.replace(/^((?:> ?)*\s*)\* /, '$1- ')
      })
      .join('\n')
      // BlockNote writes the absolute URL of an attachment image; the stored text has the path
      .split(`](${origin}/api/v1/workspaces/`).join('](/api/v1/workspaces/')
      // one blank line at most, but only outside code: the blank lines of a code block are content
      .replace(/\n{3,}/g, '\n\n')
      .replace(/^\n{2,}/, '\n')
      .replace(/\n{2,}$/, '\n'),
  )
  return text.trim()
}

type Node = Record<string, unknown>
const isNode = (value: unknown): value is Node => typeof value === 'object' && value !== null && !Array.isArray(value)

/** Rewrites the plain inline text nodes of blocks; code blocks stay. */
function mapInline(value: unknown, text: (node: Node) => unknown[], mention: (node: Node) => unknown[]): unknown {
  if (Array.isArray(value)) {
    return value.flatMap((item) => {
      if (isNode(item) && item.type === 'text' && typeof item.text === 'string') {
        // a chip has no styles: styled text (bold, inline code, …) stays text, so it keeps them
        return Object.keys((item.styles as Node | undefined) ?? {}).length > 0 ? [item] : text(item)
      }
      if (isNode(item) && item.type === 'mention') return mention(item)
      return [mapInline(item, text, mention)]
    })
  }
  if (!isNode(value) || value.type === 'codeBlock') return value
  return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, mapInline(child, text, mention)]))
}

/** Splits `@Name` out of a text node as mention chips, by the rule of the markdown renderer. */
function splitMentions(node: Node, tokens: readonly MentionToken[]): unknown[] {
  const text = node.text as string
  const labels = tokens.filter((token) => token.kind === 'user' && token.userId).sort((a, b) => b.label.length - a.label.length)
  const parts: unknown[] = []
  let from = 0
  for (let at = text.indexOf('@'); at !== -1; at = text.indexOf('@', at + 1)) {
    if (at > 0 && !isMentionBoundary(text[at - 1])) continue
    const rest = text.slice(at + 1).toLowerCase()
    const token = labels.find((candidate) => rest.startsWith(candidate.label.toLowerCase()) && isMentionBoundary(text[at + 1 + candidate.label.length]))
    const userId = token ? mentionedUserId(token.label, tokens) : null
    if (!token || !userId) continue
    if (at > from) parts.push({ ...node, text: text.slice(from, at) })
    // the text as written, so an unchanged document gives the same markdown
    parts.push({ type: 'mention', props: { userId, name: text.slice(at + 1, at + 1 + token.label.length) } })
    from = at + 1 + token.label.length
    at = from - 1
  }
  if (from === 0) return [node]
  if (from < text.length) parts.push({ ...node, text: text.slice(from) })
  return parts
}

/** The blocks of `markdown`, with the members it mentions as chips. */
export function markdownToBlocks(editor: MarkdownEditorInstance, markdown: string, tokens: readonly MentionToken[] = []) {
  const blocks = editor.tryParseMarkdownToBlocks(markdown)
  return mapInline(blocks, (node) => splitMentions(node, tokens), (node) => [node]) as typeof blocks
}

/** The markdown of the editor's document (or of `blocks`); a mention chip is written as `@Name`. */
export function blocksToMarkdown(editor: MarkdownEditorInstance, blocks: unknown[] = editor.document): string {
  const plain = mapInline(blocks, (node) => [node], (node) => [{ type: 'text', text: `@${String((node.props as Node | undefined)?.name ?? '')}`, styles: {} }])
  return normalizeMarkdown(editor.blocksToMarkdownLossy(plain as never))
}

/**
 * Whether the editor gives `markdown` back unchanged. If not (raw HTML, `__underline__`, a list in a quote, …), the
 * caller must edit the text as plain markdown: an edit in the rich editor would rewrite the parts it cannot hold.
 */
export function roundTrips(editor: MarkdownEditorInstance, markdown: string, tokens: readonly MentionToken[] = []): boolean {
  try {
    return blocksToMarkdown(editor, markdownToBlocks(editor, markdown, tokens)) === normalizeMarkdown(markdown)
  } catch {
    return false
  }
}

/**
 * `markdown` with the checklist item on line `line` (0-based, as the renderer counts lines) set to `checked`.
 * Nothing else changes. `null` when that line is not a checklist item.
 */
export function toggleChecklistLine(markdown: string, line: number, checked: boolean): string | null {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const item = /^([ \t]*[-*+]\s+\[)([ xX])(\](?:\s|$))/.exec(lines[line] ?? '')
  if (!item) return null
  lines[line] = `${item[1]}${checked ? 'x' : ' '}${item[3]}${lines[line].slice(item[0].length)}`
  return lines.join('\n')
}
