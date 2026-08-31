// Markdown rendering pipeline ported from the chat reference MessageItem.tsx (lines ~1009-1560):
// block parser (fenced code, headings, quotes, lists) -> inline markdown ->
// mentionify -> linkify. Custom regex parser, zero dependencies. Shared by
// message content and embed cards.
import { appNavigate } from '../../lib/navigateBridge'
import { CodeBlock } from './components/CodeBlock'
import { isMentionBoundary, type MentionToken } from './chatLib'

/* ---------- inline (the chat reference linkify / mentionify / renderMarkdownText) ---------- */

function safeHref(url: string): string {
  return /^https?:\/\//i.test(url) ? url : '#'
}

/** Links to this app (message links, task links…) navigate in place, Discord-style. */
function internalPath(url: string): string | null {
  const origin = window.location.origin
  return url.startsWith(`${origin}/`) ? url.slice(origin.length) : null
}

function linkifyText(text: string): React.ReactNode[] {
  const urlRegex = /https?:\/\/[^\s<]+/g
  const parts: React.ReactNode[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = urlRegex.exec(text)) !== null) {
    if (match.index > lastIndex) parts.push(text.slice(lastIndex, match.index))
    const path = internalPath(match[0])
    parts.push(
      path ? (
        <a
          key={`link-${match.index}`}
          href={path}
          className="fc-link"
          onClick={(e) => {
            e.preventDefault()
            appNavigate(path)
          }}
        >
          {match[0]}
        </a>
      ) : (
        <a
          key={`link-${match.index}`}
          href={safeHref(match[0])}
          target="_blank"
          rel="noopener noreferrer"
          className="fc-link"
        >
          {match[0]}
        </a>
      ),
    )
    lastIndex = match.index + match[0].length
  }
  if (lastIndex < text.length) parts.push(text.slice(lastIndex))
  return parts.length > 0 ? parts : [text]
}

export function mentionifyText(text: string, keyPrefix: string, mentionTokens: MentionToken[]): React.ReactNode[] {
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
      const mentionText = `${token.kind === 'channel' ? '#' : '@'}${token.label.toLowerCase()}`
      if (!text.slice(cursor).toLowerCase().startsWith(mentionText)) return false
      return isMentionBoundary(text[cursor + mentionText.length])
    })
    if (!matched) {
      const nextAt = text.indexOf('@', cursor + 1)
      const nextHash = text.indexOf('#', cursor + 1)
      const candidates = [nextAt, nextHash].filter((index) => index !== -1)
      const end = candidates.length > 0 ? Math.min(...candidates) : text.length
      parts.push(...linkifyText(text.slice(cursor, end)))
      cursor = end
      continue
    }

    const value = text.slice(cursor, cursor + matched.label.length + 1)
    const key = `${keyPrefix}-mention-${cursor}-${partIndex}`
    parts.push(
      matched.kind === 'channel' && matched.href ? (
        <a
          key={key}
          href={matched.href}
          className="fc-mention fc-mention-channel"
          style={{ color: matched.color }}
          onClick={(e) => {
            e.preventDefault()
            e.stopPropagation()
            appNavigate(matched.href!)
          }}
        >
          {value}
        </a>
      ) : (
        <span key={key} className="fc-mention" style={{ color: matched.color }}>
          {value}
        </span>
      ),
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

export function renderMarkdownText(text: string, keyPrefix: string, mentionTokens: MentionToken[] = []): React.ReactNode[] {
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
    parts.push(...mentionifyText(text.slice(lastIndex), `${keyPrefix}-${lastIndex}`, mentionTokens))
  }

  return parts.length > 0 ? parts : [text]
}

/* ---------- blocks (the chat reference renderMarkdownBlocks) ---------- */

export function renderMarkdownBlocks(text: string, keyPrefix: string, mentionTokens: MentionToken[] = []): React.ReactNode[] {
  const lines = text.replace(/\r\n/g, '\n').split('\n')
  const blocks: React.ReactNode[] = []
  let i = 0

  while (i < lines.length) {
    const line = lines[i]
    const fenceMatch = line.match(/^```([A-Za-z0-9_-]+)?\s*(.*)$/)
    if (fenceMatch) {
      const language = fenceMatch[1] || ''
      const firstLineCode = fenceMatch[2] || ''
      const codeLines: string[] = []

      if (firstLineCode.endsWith('```')) {
        codeLines.push(firstLineCode.slice(0, -3).trimEnd())
        i += 1
      } else {
        if (firstLineCode) codeLines.push(firstLineCode)
        i += 1
        while (i < lines.length && !lines[i].startsWith('```')) {
          codeLines.push(lines[i])
          i += 1
        }
        if (i < lines.length) i += 1
      }

      blocks.push(<CodeBlock key={`${keyPrefix}-code-${i}`} code={codeLines.join('\n')} language={language} />)
      continue
    }

    const headingMatch = line.match(/^(#{1,6})\s+(.+)$/)
    if (headingMatch) {
      const level = headingMatch[1].length
      blocks.push(
        <div key={`${keyPrefix}-heading-${i}`} role="heading" aria-level={level} className="fc-md-heading" data-level={level}>
          {renderMarkdownText(headingMatch[2], `${keyPrefix}-heading-${i}`, mentionTokens)}
        </div>,
      )
      i += 1
      continue
    }

    if (line.trim() === '') {
      blocks.push(<div key={`${keyPrefix}-blank-${i}`} className="fc-md-blank" />)
      i += 1
      continue
    }

    if (/^>\s?(.*)$/.test(line)) {
      const quoteLines: string[] = []
      while (i < lines.length) {
        const itemMatch = lines[i].match(/^>\s?(.*)$/)
        if (!itemMatch) break
        quoteLines.push(itemMatch[1])
        i += 1
      }
      blocks.push(
        <blockquote key={`${keyPrefix}-quote-${i}`} className="fc-md-quote">
          {quoteLines.map((quoteLine, index) => (
            <p key={`${keyPrefix}-quote-${i}-${index}`} className="fc-md-p">
              {renderMarkdownText(quoteLine, `${keyPrefix}-quote-${i}-${index}`, mentionTokens)}
            </p>
          ))}
        </blockquote>,
      )
      continue
    }

    if (/^\s*[-*+]\s+(.+)$/.test(line)) {
      const items: string[] = []
      while (i < lines.length) {
        const itemMatch = lines[i].match(/^\s*[-*+]\s+(.+)$/)
        if (!itemMatch) break
        items.push(itemMatch[1])
        i += 1
      }
      blocks.push(
        <ul key={`${keyPrefix}-ul-${i}`} className="fc-md-list" data-ordered="false">
          {items.map((item, index) => (
            <li key={`${keyPrefix}-ul-${i}-${index}`}>{renderMarkdownText(item, `${keyPrefix}-ul-${i}-${index}`, mentionTokens)}</li>
          ))}
        </ul>,
      )
      continue
    }

    if (/^\s*\d+[.)]\s+(.+)$/.test(line)) {
      const items: string[] = []
      while (i < lines.length) {
        const itemMatch = lines[i].match(/^\s*\d+[.)]\s+(.+)$/)
        if (!itemMatch) break
        items.push(itemMatch[1])
        i += 1
      }
      blocks.push(
        <ol key={`${keyPrefix}-ol-${i}`} className="fc-md-list" data-ordered="true">
          {items.map((item, index) => (
            <li key={`${keyPrefix}-ol-${i}-${index}`}>{renderMarkdownText(item, `${keyPrefix}-ol-${i}-${index}`, mentionTokens)}</li>
          ))}
        </ol>,
      )
      continue
    }

    blocks.push(
      <p key={`${keyPrefix}-p-${i}`} className="fc-md-p">
        {renderMarkdownText(line, `${keyPrefix}-p-${i}`, mentionTokens)}
      </p>,
    )
    i += 1
  }

  return blocks
}

