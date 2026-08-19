// Slash-command items — the docs reference workflow (references/the docs reference web/src/components/Editor.tsx):
// custom "Basic" items first, then the reference editor's default slash-menu items
// (titles / subtexts / aliases / groups copied from the reference editor's en dictionary).
import {
  Code,
  DocumentText,
  Link2,
  Minus,
  QuoteDown,
  Smallcaps,
  TaskSquare,
  TextalignLeft,
} from 'reicon-react'
import type { IconComponent } from 'reicon-react'
import type { DocBlock } from '../../mock/types'

export interface SlashItem {
  title: string
  subtext: string
  aliases: string[]
  group: string
  icon: IconComponent
  /** 'turn-into' replaces the current block's type; others run a custom action. */
  action:
    | { kind: 'turn-into'; type: DocBlock['type'] }
    | { kind: 'subpage' }
    | { kind: 'link-page' }
}

function heading(level: 1 | 2 | 3, subtext: string, aliases: string[]): SlashItem {
  return {
    title: `Heading ${level}`,
    subtext,
    aliases,
    group: 'Headings',
    icon: Smallcaps,
    action: { kind: 'turn-into', type: `h${level}` as DocBlock['type'] },
  }
}

export const SLASH_ITEMS: SlashItem[] = [
  // ----- the docs reference custom items (group "Basic") -----
  {
    title: 'Sub-page',
    subtext: 'Create a page under this one',
    aliases: ['subpage', 'new', 'under', 'page'],
    group: 'Basic',
    icon: DocumentText,
    action: { kind: 'subpage' },
  },
  {
    title: 'Link a page',
    subtext: 'Insert a link to an existing page',
    aliases: ['link', 'reference', 'mention'],
    group: 'Basic',
    icon: Link2,
    action: { kind: 'link-page' },
  },
  // ----- the reference editor default slash-menu items -----
  heading(1, 'Top-level heading', ['h', 'heading1', 'h1']),
  heading(2, 'Key section heading', ['h2', 'heading2', 'subheading']),
  heading(3, 'Subsection and group heading', ['h3', 'heading3', 'subheading']),
  {
    title: 'Numbered List',
    subtext: 'List with ordered items',
    aliases: ['ol', 'li', 'list', 'numberedlist', 'numbered list'],
    group: 'Basic blocks',
    icon: TextalignLeft,
    action: { kind: 'turn-into', type: 'numbered' },
  },
  {
    title: 'Bullet List',
    subtext: 'List with unordered items',
    aliases: ['ul', 'li', 'list', 'bulletlist', 'bullet list'],
    group: 'Basic blocks',
    icon: TextalignLeft,
    action: { kind: 'turn-into', type: 'bullet' },
  },
  {
    title: 'Check List',
    subtext: 'List with checkboxes',
    aliases: ['ul', 'li', 'list', 'checklist', 'check list', 'checked list', 'checkbox'],
    group: 'Basic blocks',
    icon: TaskSquare,
    action: { kind: 'turn-into', type: 'todo' },
  },
  {
    title: 'Paragraph',
    subtext: 'The body of your document',
    aliases: ['p', 'paragraph'],
    group: 'Basic blocks',
    icon: TextalignLeft,
    action: { kind: 'turn-into', type: 'p' },
  },
  {
    title: 'Quote',
    subtext: 'Quote or excerpt',
    aliases: ['quotation', 'blockquote', 'bq'],
    group: 'Basic blocks',
    icon: QuoteDown,
    action: { kind: 'turn-into', type: 'quote' },
  },
  {
    title: 'Code Block',
    subtext: 'Code block with syntax highlighting',
    aliases: ['code', 'pre'],
    group: 'Basic blocks',
    icon: Code,
    action: { kind: 'turn-into', type: 'code' },
  },
  {
    title: 'Divider',
    subtext: 'Visually divide blocks',
    aliases: ['hr', 'divider', 'separator', 'line'],
    group: 'Basic blocks',
    icon: Minus,
    action: { kind: 'turn-into', type: 'divider' },
  },
]

/** the reference editor's filterSuggestionItems: keep items whose title or any alias contains the query. */
export function filterSuggestionItems(items: SlashItem[], query: string): SlashItem[] {
  const q = query.trim().toLowerCase()
  if (!q) return items
  return items.filter(
    ({ title, aliases }) =>
      title.toLowerCase().includes(q) || aliases.some((alias) => alias.toLowerCase().includes(q)),
  )
}

/**
 * the reference editor's trigger rule: "/" typed at the start of the block or after whitespace
 * opens the menu; the query is everything between the "/" and the caret.
 */
export function getSlashState(text: string, cursor: number): { start: number; query: string } | null {
  const beforeCursor = text.slice(0, cursor)
  const slashIndex = beforeCursor.lastIndexOf('/')
  if (slashIndex === -1) return null
  const charBefore = slashIndex > 0 ? beforeCursor[slashIndex - 1] : ''
  if (charBefore && !/\s/.test(charBefore)) return null
  const query = beforeCursor.slice(slashIndex + 1)
  if (query.includes('\n') || query.length > 32) return null
  return { start: slashIndex, query }
}
