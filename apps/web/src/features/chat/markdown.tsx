// Markdown rendering pipeline ported from the chat reference MessageItem.tsx (lines ~1009-1560):
// block parser (fenced code, headings, quotes, lists) -> inline markdown ->
// mentionify -> linkify. Custom regex parser, zero dependencies. Shared by
// message content and embed cards.
import { getState } from '../../mock/store'
import { appNavigate } from '../../lib/navigateBridge'
import { CodeBlock } from './components/CodeBlock'
import { isMentionBoundary, type MentionToken } from './chatLib'

/* ---------- inline (the chat reference linkify / mentionify / renderMarkdownText) ---------- */

function safeHref(url: string): string {
  return /^https?:\/\//i.test(url) ? url : '#'
}

/** GitHub octicons for issue / pull / discussion / repo links (16×16, currentColor). */
function githubGlyph(kind: 'issue' | 'pull' | 'discussion' | 'repo'): React.ReactNode {
  const paths: Record<string, string> = {
    issue:
      'M8 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0Z',
    pull: 'M1.5 3.25a2.25 2.25 0 1 1 3 2.122v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.25 2.25 0 0 1 1.5 3.25Zm5.677-.177L9.573.677A.25.25 0 0 1 10 .854V2.5h1A2.5 2.5 0 0 1 13.5 5v5.628a2.251 2.251 0 1 1-1.5 0V5a1 1 0 0 0-1-1h-1v1.646a.25.25 0 0 1-.427.177L7.177 3.427a.25.25 0 0 1 0-.354ZM3.75 2.5a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Zm0 9.5a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Zm8.25.75a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Z',
    discussion:
      'M1.75 1h8.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 10.25 10H7.061l-2.574 2.573A1.458 1.458 0 0 1 2 11.543V10h-.25A1.75 1.75 0 0 1 0 8.25v-5.5C0 1.784.784 1 1.75 1ZM1.5 2.75v5.5c0 .138.112.25.25.25h1a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h3.5a.25.25 0 0 0 .25-.25v-5.5a.25.25 0 0 0-.25-.25h-8.5a.25.25 0 0 0-.25.25Zm13 2a.25.25 0 0 0-.25-.25h-.5a.75.75 0 0 1 0-1.5h.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 14.25 12H14v1.543a1.458 1.458 0 0 1-2.487 1.03L9.22 12.28a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215l2.22 2.22v-2.19a.75.75 0 0 1 .75-.75h1a.25.25 0 0 0 .25-.25Z',
    repo: 'M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z',
  }
  return (
    <svg className={`fc-gh-icon fc-gh-${kind}`} width="14" height="14" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <path d={paths[kind]} />
    </svg>
  )
}

/** Bare github.com URLs render like GitHub does: state icon + "owner/repo#123" (or "owner/repo"). */
function githubLink(url: string): { kind: 'issue' | 'pull' | 'discussion' | 'repo'; label: string } | null {
  const match = url.match(/^https?:\/\/github\.com\/([\w.-]+)\/([\w.-]+?)(?:\/(issues|pull|discussions)\/(\d+))?(?:[/?#].*)?$/)
  if (!match) return null
  const [, owner, repo, section, number] = match
  if (section && number) {
    const kind = section === 'pull' ? 'pull' : section === 'discussions' ? 'discussion' : 'issue'
    return { kind, label: `${owner}/${repo}#${number}` }
  }
  return { kind: 'repo', label: `${owner}/${repo}` }
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
    const github = path ? null : githubLink(match[0])
    parts.push(
      github ? (
        <a
          key={`link-${match.index}`}
          href={safeHref(match[0])}
          target="_blank"
          rel="noopener noreferrer"
          className="fc-link fc-github-link"
          title={match[0]}
        >
          {githubGlyph(github.kind)}
          {github.label}
        </a>
      ) : path ? (
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
      const custom = getState().customEmojis.find((e) => e.name === shortcode)
      if (custom) {
        parts.push(<img key={key} className="fc-custom-emoji" src={custom.url} alt={token} title={token} />)
      } else {
        parts.push(EMOJI_SHORTCODES[shortcode] || token)
      }
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

