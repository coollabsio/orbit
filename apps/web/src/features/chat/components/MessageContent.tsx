// Rendering pipeline copied from the chat reference MessageItem.tsx:
// linkify -> mentionify -> markdown-lite, plus the MessageContent component.
import { isMentionBoundary, type MentionToken } from '../chatLib'

/* ---------- rendering (the chat reference linkify/mentionify/markdown) ---------- */

function safeHref(url: string): string {
  return /^https?:\/\//i.test(url) ? url : '#'
}

function linkifyText(text: string): React.ReactNode[] {
  const urlRegex = /https?:\/\/[^\s<]+/g
  const parts: React.ReactNode[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = urlRegex.exec(text)) !== null) {
    if (match.index > lastIndex) parts.push(text.slice(lastIndex, match.index))
    parts.push(
      <a
        key={`link-${match.index}`}
        href={safeHref(match[0])}
        target="_blank"
        rel="noopener noreferrer"
        className="fc-link"
      >
        {match[0]}
      </a>,
    )
    lastIndex = match.index + match[0].length
  }
  if (lastIndex < text.length) parts.push(text.slice(lastIndex))
  return parts.length > 0 ? parts : [text]
}

function mentionifyText(text: string, keyPrefix: string, mentionTokens: MentionToken[]): React.ReactNode[] {
  if (mentionTokens.length === 0) return linkifyText(text)

  const sortedTokens = [...mentionTokens]
    .filter(
      (token, index, arr) =>
        token.label && arr.findIndex((other) => other.label.toLowerCase() === token.label.toLowerCase()) === index,
    )
    .sort((left, right) => right.label.length - left.label.length)
  const parts: React.ReactNode[] = []
  let cursor = 0
  let partIndex = 0

  while (cursor < text.length) {
    const matched = sortedTokens.find((token) => {
      const mentionText = `@${token.label.toLowerCase()}`
      if (!text.slice(cursor).toLowerCase().startsWith(mentionText)) return false
      return isMentionBoundary(text[cursor + mentionText.length])
    })
    if (!matched) {
      const nextAt = text.indexOf('@', cursor + 1)
      const end = nextAt === -1 ? text.length : nextAt
      parts.push(...linkifyText(text.slice(cursor, end)))
      cursor = end
      continue
    }

    const value = text.slice(cursor, cursor + matched.label.length + 1)
    parts.push(
      <span
        key={`${keyPrefix}-mention-${cursor}-${partIndex}`}
        className="fc-mention"
        style={{ color: matched.color }}
      >
        {value}
      </span>,
    )
    cursor += matched.label.length + 1
    partIndex += 1
  }

  return parts.length > 0 ? parts : [text]
}

const EMOJI_SHORTCODES: Record<string, string> = {
  white_check_mark: '✅',
  check: '✅',
  heavy_check_mark: '✔️',
  x: '❌',
  cross_mark: '❌',
  crossmark: '❌',
  warning: '⚠️',
  information_source: 'ℹ️',
}

function renderMarkdownText(text: string, keyPrefix: string, mentionTokens: MentionToken[]): React.ReactNode[] {
  const tokenRegex =
    /(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\)|:[a-z0-9_+-]+:|`[^`]+`|\*\*[^*]+?\*\*|__[^_]+?__|\*[^*\s][^*]*?\*|_[^_\s][^_]*?_)/g
  const parts: React.ReactNode[] = []
  let lastIndex = 0
  let tokenIndex = 0
  let match: RegExpExecArray | null

  while ((match = tokenRegex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(...mentionifyText(text.slice(lastIndex, match.index), `${keyPrefix}-${lastIndex}`, mentionTokens))
    }

    const token = match[0]
    const key = `${keyPrefix}-${match.index}-${tokenIndex}`
    if (token.startsWith('[')) {
      const linkMatch = token.match(/^\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)$/)
      if (linkMatch) {
        parts.push(
          <a key={key} href={safeHref(linkMatch[2])} target="_blank" rel="noopener noreferrer" className="fc-link">
            {renderMarkdownText(linkMatch[1], `${key}-link`, mentionTokens)}
          </a>,
        )
      } else {
        parts.push(token)
      }
    } else if (token.startsWith('`')) {
      parts.push(
        <code key={key} className="fc-code">
          {token.slice(1, -1)}
        </code>,
      )
    } else if (token.startsWith(':')) {
      const shortcode = token.slice(1, -1)
      parts.push(EMOJI_SHORTCODES[shortcode] || token)
    } else if (token.startsWith('**') || token.startsWith('__')) {
      parts.push(<strong key={key}>{renderMarkdownText(token.slice(2, -2), `${key}-strong`, mentionTokens)}</strong>)
    } else {
      parts.push(<em key={key}>{renderMarkdownText(token.slice(1, -1), `${key}-em`, mentionTokens)}</em>)
    }

    lastIndex = match.index + token.length
    tokenIndex += 1
  }

  if (lastIndex < text.length) {
    parts.push(...mentionifyText(text.slice(lastIndex), `${keyPrefix}-tail`, mentionTokens))
  }

  return parts
}

export function MessageContent({ content, mentionTokens }: { content: string; mentionTokens: MentionToken[] }) {
  if (!content.trim()) return null
  return <div className="fc-msg-text">{renderMarkdownText(content, 'message', mentionTokens)}</div>
}

