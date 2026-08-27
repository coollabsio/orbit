// Port of the chat reference DiscordEmbeds / DiscordEmbedCard (MessageItem.tsx L616-770):
// up to 10 cards, 4px accent left border, author/title/description/fields/image/footer + thumbnail.
import type { Embed } from '../../../mock/types'
import type { MentionToken } from '../chatLib'
import { renderMarkdownBlocks, renderMarkdownText } from '../markdown'

function hasVisibleEmbedContent(embed: Embed): boolean {
  return Boolean(
    embed.title ||
      embed.description ||
      embed.author?.name ||
      embed.footer?.text ||
      embed.image?.url ||
      embed.thumbnail?.url ||
      embed.fields?.some((field) => field.name || field.value),
  )
}

function embedColor(color?: number): string {
  if (!Number.isInteger(color) || color! < 0 || color! > 0xffffff) return '#5865f2'
  return `#${color!.toString(16).padStart(6, '0')}`
}

function safeImageUrl(url?: string): string | undefined {
  if (!url || !/^https?:\/\//i.test(url)) return undefined
  return url
}

function safeHref(url?: string): string {
  return url && /^https?:\/\//i.test(url) ? url : '#'
}

function formatEmbedTimestamp(timestamp?: string): string | null {
  if (!timestamp) return null
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return null
  return date.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

function EmbedLinkedText({ href, className, children }: { href?: string; className: string; children: React.ReactNode }) {
  const safe = safeHref(href)
  if (!href || safe === '#') return <div className={className}>{children}</div>
  return (
    <a href={safe} target="_blank" rel="noopener noreferrer" className={className} data-link="true">
      {children}
    </a>
  )
}

export function EmbedCard({ embed, mentionTokens = [] }: { embed: Embed; mentionTokens?: MentionToken[] }) {
  const accent = embedColor(embed.color)
  const thumbnailUrl = safeImageUrl(embed.thumbnail?.url)
  const imageUrl = safeImageUrl(embed.image?.url)
  const authorIconUrl = safeImageUrl(embed.author?.iconUrl)
  const footerIconUrl = safeImageUrl(embed.footer?.iconUrl)
  const timestamp = formatEmbedTimestamp(embed.timestamp)

  return (
    <div className="fc-embed" style={{ borderLeftColor: accent }}>
      <div className="fc-embed-inner">
        <div className="fc-embed-main">
          {embed.author?.name ? (
            <EmbedLinkedText href={embed.author.url} className="fc-embed-author">
              {authorIconUrl ? <img src={authorIconUrl} alt="" /> : null}
              <span className="truncate">{embed.author.name}</span>
            </EmbedLinkedText>
          ) : null}
          {embed.title ? (
            <EmbedLinkedText href={embed.url} className="fc-embed-title">
              {renderMarkdownText(embed.title, 'embed-title', mentionTokens)}
            </EmbedLinkedText>
          ) : null}
          {embed.description ? (
            <div className="fc-embed-desc">{renderMarkdownBlocks(embed.description, 'embed-description', mentionTokens)}</div>
          ) : null}
          {embed.fields && embed.fields.length > 0 ? (
            <div className="fc-embed-fields">
              {embed.fields.slice(0, 25).map((field, index) => (
                <div key={`${field.name || 'field'}-${index}`} className="fc-embed-field" data-inline={field.inline ? 'true' : 'false'}>
                  {field.name ? (
                    <div className="fc-embed-field-name">{renderMarkdownText(field.name, `embed-field-name-${index}`, mentionTokens)}</div>
                  ) : null}
                  {field.value ? (
                    <div className="fc-embed-field-value">
                      {renderMarkdownBlocks(field.value, `embed-field-value-${index}`, mentionTokens)}
                    </div>
                  ) : null}
                </div>
              ))}
            </div>
          ) : null}
          {imageUrl ? (
            <a href={imageUrl} target="_blank" rel="noopener noreferrer" className="fc-embed-image">
              <img src={imageUrl} alt="" loading="lazy" />
            </a>
          ) : null}
          {embed.footer?.text || timestamp ? (
            <div className="fc-embed-footer">
              {footerIconUrl ? <img src={footerIconUrl} alt="" /> : null}
              <span className="truncate">{[embed.footer?.text, timestamp].filter(Boolean).join(' - ')}</span>
            </div>
          ) : null}
        </div>
        {thumbnailUrl ? (
          <a href={thumbnailUrl} target="_blank" rel="noopener noreferrer" className="fc-embed-thumb">
            <img src={thumbnailUrl} alt="" loading="lazy" />
          </a>
        ) : null}
      </div>
    </div>
  )
}

export function EmbedCards({
  embeds,
  hasTextContent = true,
  mentionTokens = [],
}: {
  embeds: Embed[]
  hasTextContent?: boolean
  mentionTokens?: MentionToken[]
}) {
  const visible = embeds.filter(hasVisibleEmbedContent).slice(0, 10)
  if (visible.length === 0) return null
  return (
    <div className="fc-embeds" data-no-text={hasTextContent ? undefined : 'true'}>
      {visible.map((embed, index) => (
        <EmbedCard key={index} embed={embed} mentionTokens={mentionTokens} />
      ))}
    </div>
  )
}
