// Plain-text previews of markdown message content, for lists and thread cards.

/** Plain one-line preview of a markdown message (the chat reference extractPreview) for thread cards and lists. */
export function extractPreview(content: string): string {
  const line = content
    .replace(/\r\n/g, '\n')
    .split('\n')
    .map((l) => l.trim())
    .find((l) => l && !l.startsWith('```'))
  if (!line) return ''
  return line
    .replace(/^#{1,6}\s+/, '')
    .replace(/^>\s?/, '')
    .replace(/^\s*(?:[-*+]|\d+[.)])\s+/, '')
    .replace(/\*\*([^*]+)\*\*|__([^_]+)__|\*([^*]+)\*|_([^_]+)_|`([^`]+)`/g, (_, a, b, c, d, e) => a ?? b ?? c ?? d ?? e)
    .replace(/\[([^\]]+)\]\(https?:\/\/[^\s)]+\)/g, '$1')
}
