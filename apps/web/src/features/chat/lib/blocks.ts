import type { BlockText, RichElement, RichInline, TextStyle } from '@orbit/api/generated/types.gen'

/** Markdown characters that a backslash keeps as text. `:` stays, so `:name:` is still an emoji, as in Slack. */
const MARKDOWN_SPECIAL = /[\\`*_~[\]|<>#!-]/g

/** Text that shows as it is when rendered as Markdown. */
export function escapeMarkdown(text: string): string {
  return text.replace(MARKDOWN_SPECIAL, '\\$&')
}

/** A block text object as Markdown: `mrkdwn` is Markdown already, `plain_text` is escaped. */
export function blockTextMarkdown(text: BlockText): string {
  return text.type === 'mrkdwn' ? text.text : escapeMarkdown(text.text)
}

/** Wraps each line of `text` in the style's markers, with its edge spaces outside them (`** x**` is no bold). */
function styled(text: string, style: TextStyle | undefined): string {
  if (!style) return escapeMarkdown(text)
  return text
    .split('\n')
    .map((line) => {
      const [, lead, core, trail] = line.match(/^(\s*)(.*?)(\s*)$/) ?? ['', '', line, '']
      if (!core) return line
      if (style.code) return `${lead}\`${core.replace(/`/g, "'")}\`${trail}`
      let out = escapeMarkdown(core)
      if (style.strike) out = `~~${out}~~`
      if (style.italic) out = `_${out}_`
      if (style.bold) out = `**${out}**`
      return `${lead}${out}${trail}`
    })
    .join('\n')
}

function inline(elements: readonly RichInline[]): string {
  return elements
    .map((element) => {
      switch (element.type) {
        case 'text':
          return styled(element.text, element.style)
        case 'link': {
          const label = escapeMarkdown(element.text || element.url)
          // The renderer links http(s) only; a mailto link shows its text and address.
          return element.url.startsWith('mailto:') ? `${label} (${escapeMarkdown(element.url.slice(7))})` : `[${label}](${element.url})`
        }
        case 'emoji':
          return `:${element.name}:`
        case 'user':
          return `<@${element.user_id}>`
        case 'channel':
          return `<#${element.channel_id}>`
        case 'broadcast':
          return element.range === 'here' ? '<!here>' : '<!channel>'
      }
    })
    .join('')
}

/**
 * A Slack `rich_text` block as Orbit Markdown with mention tokens, so it renders like a message body: sections are
 * paragraphs, lists keep their indent, quotes are `>` lines and preformatted runs are a code block.
 */
export function richTextMarkdown(elements: readonly RichElement[]): string {
  return elements
    .map((element) => {
      switch (element.type) {
        case 'rich_text_section':
          return inline(element.elements)
        case 'rich_text_quote':
          return inline(element.elements)
            .split('\n')
            .map((line) => `> ${line}`)
            .join('\n')
        case 'rich_text_preformatted':
          return `\`\`\`\n${element.elements.map((part) => (part.type === 'text' ? part.text : part.type === 'link' ? (part.text ?? part.url) : '')).join('')}\n\`\`\``
        case 'rich_text_list': {
          const pad = '  '.repeat(element.indent ?? 0)
          return element.elements
            .map((item, index) => `${pad}${element.style === 'ordered' ? `${index + 1}.` : '-'} ${inline(item.elements).replace(/\n/g, ' ')}`)
            .join('\n')
        }
      }
    })
    .join('\n')
}
