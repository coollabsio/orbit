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
    .replace(/\[([^\]]+)\]\(https?:\/\/[^\s)]+\)/g, '$1')
    // `<https://x>` is the URL without a card.
    .replace(/<(https?:\/\/[^\s<>]+)>/g, '$1')
    // A backslash before punctuation only keeps it text (`\:D`, `\*`): it goes, and what it escapes opens no emphasis.
    .replace(/\\([!-/:-@[-`{-~])|\*\*([^*]+)\*\*|__([^_]+)__|\*([^*]+)\*|_([^_]+)_|`([^`]+)`/g, (_, x, a, b, c, d, e) => x ?? a ?? b ?? c ?? d ?? e)
}

/**
 * What a one-line preview says for a message without text: `Sticker` for a sticker, else that a file was sent (a
 * message has text, a sticker or files).
 */
export function wordlessPreview(sticker: boolean): string {
  return sticker ? 'Sticker' : 'Sent a file'
}
