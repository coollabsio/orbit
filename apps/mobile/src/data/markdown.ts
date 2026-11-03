/**
 * Markdown (task descriptions, comments, chat) to readable plain text: paragraphs and list bullets stay, the markup
 * goes. Code keeps its text as written.
 */
export function markdownToText(markdown: string): string {
  const out: string[] = []
  let inFence = false
  for (const line of markdown.replace(/\r\n?/g, '\n').split('\n')) {
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence
      continue
    }
    if (inFence) {
      out.push(line)
      continue
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      out.push('')
      continue
    }
    out.push(inline(block(line)))
  }
  return out
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function block(line: string): string {
  const heading = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/.exec(line)
  if (heading) return heading[1]!
  const quote = /^\s*>\s?(.*)$/.exec(line)
  if (quote) return block(quote[1]!)
  const task = /^(\s*)[-*+]\s+\[([ xX])\]\s+(.*)$/.exec(line)
  if (task) return `${task[1]}${task[2] === ' ' ? '☐' : '☑'} ${task[3]}`
  const bullet = /^(\s*)[-*+]\s+(.*)$/.exec(line)
  if (bullet) return `${bullet[1]}• ${bullet[2]}`
  return line
}

function inline(text: string): string {
  // Code spans first, so nothing inside them is read as markup.
  return text
    .split(/(`[^`]+`)/)
    .map((part, index) =>
      index % 2 === 1
        ? part.slice(1, -1)
        : part
            .replace(/!\[([^\]]*)\]\([^)]*\)/g, (_match, alt: string) => alt || 'Image')
            .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
            .replace(/<((?:https?|mailto):[^>\s]+)>/g, '$1')
            .replace(/(\*\*|__)(?=\S)(.+?)(?<=\S)\1/g, '$2')
            .replace(/~~(?=\S)(.+?)(?<=\S)~~/g, '$1')
            .replace(/(^|[^\w*])\*(?=\S)(.+?)(?<=\S)\*(?![\w*])/g, '$1$2')
            .replace(/(^|[^\w])_(?=\S)(.+?)(?<=\S)_(?!\w)/g, '$1$2'),
    )
    .join('')
}
