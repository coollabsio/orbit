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
    <a href={safe} target="_blank" rel="noopener noreferrer" className={`${className} text-primary hover:underline`} data-link="true">
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
    <div className="overflow-hidden rounded border border-l-4 border-border bg-muted/35 text-sm text-foreground/90 max-[899px]:text-xs" style={{ borderLeftColor: accent }}>
      <div className="flex gap-4 p-3 max-[899px]:gap-2.5 max-[899px]:p-2">
        <div className="flex min-w-0 flex-1 flex-col gap-2 max-[899px]:gap-1.5">
          {embed.author?.name ? (
            <EmbedLinkedText href={embed.author.url} className="flex min-w-0 items-center gap-2 text-xs font-bold text-foreground">
              {authorIconUrl ? <img src={authorIconUrl} alt="" className="size-5 shrink-0 rounded-full object-cover" /> : null}
              <span className="truncate">{embed.author.name}</span>
            </EmbedLinkedText>
          ) : null}
          {embed.title ? (
            <EmbedLinkedText href={embed.url} className="block text-sm font-bold text-foreground max-[899px]:text-xs">
              {renderMarkdownText(embed.title, 'embed-title', mentionTokens)}
            </EmbedLinkedText>
          ) : null}
          {embed.description ? (
            <div className="text-[13px] leading-5 font-semibold whitespace-pre-wrap text-foreground/90 max-[899px]:text-[11px] max-[899px]:leading-4">{renderMarkdownBlocks(embed.description, 'embed-description', mentionTokens)}</div>
          ) : null}
          {embed.fields && embed.fields.length > 0 ? (
            <div className="grid grid-cols-3 gap-2 max-[899px]:gap-1.5 max-[599px]:grid-cols-1">
              {embed.fields.slice(0, 25).map((field, index) => (
                <div key={`${field.name || 'field'}-${index}`} className={`min-w-0 ${field.inline ? '' : 'col-span-3 max-[599px]:col-auto'}`} data-inline={field.inline ? 'true' : 'false'}>
                  {field.name ? (
                    <div className="text-xs font-bold text-foreground max-[899px]:text-[10px] max-[899px]:leading-[15px]">{renderMarkdownText(field.name, `embed-field-name-${index}`, mentionTokens)}</div>
                  ) : null}
                  {field.value ? (
                    <div className="mt-0.5 text-xs leading-5 font-medium whitespace-pre-wrap text-foreground/85 max-[899px]:text-[10px] max-[899px]:leading-[15px]">
                      {renderMarkdownBlocks(field.value, `embed-field-value-${index}`, mentionTokens)}
                    </div>
                  ) : null}
                </div>
              ))}
            </div>
          ) : null}
          {imageUrl ? (
            <a href={imageUrl} target="_blank" rel="noopener noreferrer" className="block w-fit overflow-hidden rounded border border-border transition-opacity hover:opacity-90">
              <img src={imageUrl} alt="" loading="lazy" className="block max-h-80 max-w-full object-contain" />
            </a>
          ) : null}
          {embed.footer?.text || timestamp ? (
            <div className="flex min-w-0 items-center gap-2 text-[11px] font-semibold text-muted-foreground max-[899px]:gap-[5px] max-[899px]:text-[9px]">
              {footerIconUrl ? <img src={footerIconUrl} alt="" className="size-4 shrink-0 rounded-full object-cover" /> : null}
              <span className="truncate">{[embed.footer?.text, timestamp].filter(Boolean).join(' - ')}</span>
            </div>
          ) : null}
        </div>
        {thumbnailUrl ? (
          <a href={thumbnailUrl} target="_blank" rel="noopener noreferrer" className="block size-20 shrink-0 overflow-hidden rounded border border-border transition-opacity hover:opacity-90">
            <img src={thumbnailUrl} alt="" loading="lazy" className="size-full object-cover" />
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
    <div className={`flex max-w-[520px] flex-col gap-2 max-[899px]:max-w-[320px] max-[899px]:gap-[5px] ${hasTextContent ? 'mt-2' : 'mt-1'} max-[899px]:mt-[5px]`} data-no-text={hasTextContent ? undefined : 'true'}>
      {visible.map((embed, index) => (
        <EmbedCard key={index} embed={embed} mentionTokens={mentionTokens} />
      ))}
    </div>
  )
}
