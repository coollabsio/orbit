import type { ReactNode } from 'react'
import { TaskMentionChip } from './TaskChip'

export type { TaskChipLookup } from './taskChipContext'

type Attrs = Record<string, unknown> | undefined
interface NodeProps {
  node: { attrs?: Attrs; text?: string }
  children?: ReactNode
}
interface MarkProps {
  mark: { attrs?: Attrs }
  children?: ReactNode
}

function safeHref(href: unknown): string {
  const value = typeof href === 'string' ? href : ''
  return value.startsWith('http://') || value.startsWith('https://') ? value : '#'
}

/**
 * The static renderer does not run node views, so every node needs an entry
 * here. `mention` and `taskMention` are the two that would otherwise render as
 * nothing. Unknown nodes and marks fall back to their children (see
 * `RichTextView`), so a document from a newer server never crashes the page.
 */
export const nodeMapping = {
  doc: ({ children }: NodeProps) => <>{children}</>,
  paragraph: ({ children }: NodeProps) => <p>{children}</p>,
  text: ({ node }: NodeProps) => <>{node.text ?? ''}</>,
  heading: ({ node, children }: NodeProps) => {
    const level = Math.min(Math.max(Number(node.attrs?.level ?? 1) || 1, 1), 3)
    const Tag = `h${level}` as 'h1' | 'h2' | 'h3'
    return <Tag>{children}</Tag>
  },
  bulletList: ({ children }: NodeProps) => <ul>{children}</ul>,
  orderedList: ({ node, children }: NodeProps) => {
    const start = Number(node.attrs?.start)
    return <ol start={Number.isInteger(start) && start > 1 ? start : undefined}>{children}</ol>
  },
  listItem: ({ children }: NodeProps) => <li>{children}</li>,
  taskList: ({ children }: NodeProps) => <ul className="editor-task-list">{children}</ul>,
  taskItem: ({ node, children }: NodeProps) => (
    <li className="editor-task-item" data-checked={node.attrs?.checked ? 'true' : undefined}>
      <input type="checkbox" checked={Boolean(node.attrs?.checked)} readOnly tabIndex={-1} aria-hidden="true" />
      <div>{children}</div>
    </li>
  ),
  blockquote: ({ children }: NodeProps) => <blockquote>{children}</blockquote>,
  codeBlock: ({ node, children }: NodeProps) => (
    <pre data-language={typeof node.attrs?.language === 'string' ? node.attrs.language : undefined}>
      <code>{children}</code>
    </pre>
  ),
  horizontalRule: () => <hr />,
  hardBreak: () => <br />,
  mention: ({ node }: NodeProps) => (
    <span className="editor-mention">{`@${String(node.attrs?.label ?? 'unknown')}`}</span>
  ),
  taskMention: TaskMentionChip,
}

export const markMapping = {
  bold: ({ children }: MarkProps) => <strong>{children}</strong>,
  italic: ({ children }: MarkProps) => <em>{children}</em>,
  strike: ({ children }: MarkProps) => <s>{children}</s>,
  code: ({ children }: MarkProps) => <code>{children}</code>,
  underline: ({ children }: MarkProps) => <u>{children}</u>,
  link: ({ mark, children }: MarkProps) => (
    <a href={safeHref(mark.attrs?.href)} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  ),
}

export const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>
