import type { ActionElement, AttachmentBlock, ContextElement, MessageBlock } from '@orbit/api/generated/types.gen'
import { createContext, useContext, useEffect, useState } from 'react'
import { buttonVariants } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { renderMarkdownBlocks } from '@/lib/markdown'
import { blockTextMarkdown, escapeMarkdown, richTextMarkdown } from '../../lib/blocks'
import { emojiNamed, type EmojiShortcode, loadEmojiIndex } from '../../lib/emojiSearch'
import { decodeMentions } from '../../lib/mentionTokens'
import { fullTimestamp } from '../../lib/time'
import { useChatPeople } from './people'

const SHORTCODE = /:([a-z0-9_+-]+):/g

/**
 * The standard emoji index, once a block uses a `:name:`. Integrations send names (`:tada:`) where the composer would
 * have put the emoji itself; the Markdown renderer knows only a few names, and custom emoji by their own context.
 */
const EmojiIndexContext = createContext<readonly EmojiShortcode[] | null>(null)

function withEmoji(text: string, index: readonly EmojiShortcode[] | null): string {
  return index ? text.replace(SHORTCODE, (code, name: string) => emojiNamed(index, name) ?? code) : text
}

/** Markdown with mention tokens, rendered as a message body is. */
function Markdown({ text, id, className }: { text: string; id: string; className?: string }) {
  const people = useChatPeople()
  const decoded = decodeMentions(withEmoji(text, useContext(EmojiIndexContext)), people.members, people.channels)
  return (
    <div className={className ?? 'leading-[1.5] wrap-anywhere text-foreground/85 [&_pre]:overflow-x-auto [&_pre]:whitespace-pre'}>
      {renderMarkdownBlocks(decoded, id, people.tokens, { chat: true })}
    </div>
  )
}

function ContextLine({ elements, id }: { elements: ContextElement[]; id: string }) {
  return (
    <div data-slot="message-block-context" className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
      {elements.map((element, index) =>
        element.type === 'image' ? (
          <img
            key={index}
            src={element.image_url}
            alt={element.alt_text}
            title={element.alt_text}
            referrerPolicy="no-referrer"
            className="size-4 rounded-sm object-cover"
          />
        ) : (
          <Markdown
            key={index}
            id={`${id}-${index}`}
            text={element.type === 'mrkdwn' ? element.text : escapeMarkdown(element.text)}
            className="min-w-0 wrap-anywhere [&>p]:inline"
          />
        ),
      )}
    </div>
  )
}

function ExternalLink({ href, children, className }: { href: string; children: React.ReactNode; className?: string }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" className={className ?? 'text-primary hover:underline'}>
      {children}
    </a>
  )
}

/** A Slack legacy attachment: pretext, then a box with a colored bar, author, title, text, fields, image and footer. */
function AttachmentBox({ attachment, id }: { attachment: AttachmentBlock; id: string }) {
  const { author_name, author_link, author_icon, title, title_link, footer, footer_icon, ts } = attachment
  return (
    <div data-slot="message-block-attachment" className="grid min-w-0 gap-1.5">
      {attachment.pretext ? <Markdown text={attachment.pretext} id={`${id}-pre`} /> : null}
      <div className="flex min-w-0 gap-3">
        <span
          aria-hidden="true"
          className="w-1 shrink-0 rounded-full bg-border"
          style={attachment.color ? { backgroundColor: attachment.color } : undefined}
        />
        <div className="grid min-w-0 flex-1 gap-1.5">
          <div className="flex min-w-0 gap-3">
            <div className="grid min-w-0 flex-1 gap-1">
              {author_name ? (
                <div className="flex min-w-0 items-center gap-1.5 text-sm text-muted-foreground">
                  {author_icon ? <img src={author_icon} alt="" referrerPolicy="no-referrer" className="size-4 rounded-sm object-cover" /> : null}
                  {author_link ? (
                    <ExternalLink href={author_link} className="truncate hover:underline">
                      {author_name}
                    </ExternalLink>
                  ) : (
                    <span className="truncate">{author_name}</span>
                  )}
                </div>
              ) : null}
              {title ? (
                <p className="font-semibold wrap-anywhere text-foreground">
                  {title_link ? <ExternalLink href={title_link}>{title}</ExternalLink> : title}
                </p>
              ) : null}
              {attachment.text ? <Markdown text={attachment.text} id={`${id}-text`} /> : null}
            </div>
            {attachment.thumb_url ? (
              <img src={attachment.thumb_url} alt="" referrerPolicy="no-referrer" className="size-16 shrink-0 rounded-md object-cover" />
            ) : null}
          </div>
          {attachment.fields && attachment.fields.length > 0 ? (
            <div className="grid grid-cols-2 gap-x-4 gap-y-2">
              {attachment.fields.map((field, index) => (
                <div key={index} className={field.short ? 'min-w-0 max-[599px]:col-span-2' : 'col-span-2 min-w-0'}>
                  {field.title ? <p className="text-sm font-semibold text-foreground">{field.title}</p> : null}
                  {field.value ? <Markdown text={field.value} id={`${id}-f${index}`} /> : null}
                </div>
              ))}
            </div>
          ) : null}
          {attachment.image_url ? (
            <img
              src={attachment.image_url}
              alt=""
              referrerPolicy="no-referrer"
              className="max-h-[360px] max-w-full justify-self-start rounded-md border object-contain"
            />
          ) : null}
          {footer || ts ? (
            <div className="flex min-w-0 flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
              {footer_icon ? <img src={footer_icon} alt="" referrerPolicy="no-referrer" className="size-4 rounded-sm object-cover" /> : null}
              {footer ? <Markdown text={footer} id={`${id}-foot`} className="min-w-0 wrap-anywhere [&>p]:inline" /> : null}
              {footer && ts ? <span aria-hidden="true">·</span> : null}
              {ts ? <time dateTime={new Date(ts * 1000).toISOString()}>{fullTimestamp(ts * 1000)}</time> : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}

/**
 * A Block Kit button. With a `url` it opens the link. Without one it would send its action to the integration, which
 * Orbit cannot do yet: it shows disabled and says so.
 */
function BlockButton({ element }: { element: ActionElement }) {
  const variant = element.style === 'danger' ? 'destructive' : element.style === 'primary' ? 'default' : 'outline'
  const className = buttonVariants({ variant, size: 'sm', className: 'max-w-full' })
  const label = <span className="truncate">{element.text.text}</span>
  if (element.url) {
    return (
      <a data-slot="message-block-button" href={element.url} target="_blank" rel="noopener noreferrer" className={className}>
        {label}
      </a>
    )
  }
  return (
    <span title="This button sends an action to the integration. Orbit does not support that yet." className="inline-flex max-w-full">
      <button data-slot="message-block-button" type="button" disabled className={className}>
        {label}
      </button>
    </span>
  )
}

function Block({ block, id }: { block: MessageBlock; id: string }) {
  switch (block.type) {
    case 'header':
      return (
        <div data-slot="message-block-header">
          <Markdown text={escapeMarkdown(block.text.text)} id={id} className="text-base font-semibold wrap-anywhere text-foreground" />
        </div>
      )
    case 'section':
      return (
        <div data-slot="message-block-section" className="flex min-w-0 items-start gap-3">
          <div className="grid min-w-0 flex-1 gap-2">
            {block.text ? <Markdown text={blockTextMarkdown(block.text)} id={id} /> : null}
            {block.fields && block.fields.length > 0 ? (
              <div className="grid grid-cols-2 gap-x-4 gap-y-2 max-[599px]:grid-cols-1">
                {block.fields.map((field, index) => (
                  <Markdown key={index} text={blockTextMarkdown(field)} id={`${id}-f${index}`} />
                ))}
              </div>
            ) : null}
          </div>
          {block.accessory ? <BlockButton element={block.accessory} /> : null}
        </div>
      )
    case 'actions':
      return (
        <div data-slot="message-block-actions" className="flex flex-wrap gap-2">
          {block.elements.map((element, index) => (
            <BlockButton key={index} element={element} />
          ))}
        </div>
      )
    case 'context':
      return <ContextLine elements={block.elements} id={id} />
    case 'image':
      return (
        <figure data-slot="message-block-image" className="grid gap-1">
          {block.title ? (
            <figcaption>
              <Markdown text={escapeMarkdown(block.title.text)} id={`${id}-title`} className="text-sm text-muted-foreground" />
            </figcaption>
          ) : null}
          <img
            src={block.image_url}
            alt={block.alt_text}
            title={block.alt_text}
            referrerPolicy="no-referrer"
            className="max-h-[360px] max-w-full justify-self-start rounded-md border object-contain"
          />
        </figure>
      )
    case 'divider':
      return <Separator data-slot="message-block-divider" className="my-1" />
    case 'markdown':
      return <Markdown text={block.text} id={id} />
    case 'rich_text':
      return <Markdown text={richTextMarkdown(block.elements)} id={id} />
    case 'attachment':
      return <AttachmentBox attachment={block} id={id} />
  }
}

/** An integration's layout blocks, as Slack shows them: stacked in the message, in place of its text. */
export function MessageBlocks({ blocks, messageId }: { blocks: MessageBlock[]; messageId: string }) {
  const [emojiIndex, setEmojiIndex] = useState<readonly EmojiShortcode[] | null>(null)
  const usesNames = /:[a-z0-9_+-]+:/.test(JSON.stringify(blocks))
  useEffect(() => {
    if (usesNames) void loadEmojiIndex().then(setEmojiIndex, () => {})
  }, [usesNames])
  return (
    <EmojiIndexContext value={emojiIndex}>
      <div data-slot="message-blocks" className="grid max-w-[600px] min-w-0 gap-2">
        {blocks.map((block, index) => (
          <Block key={index} block={block} id={`${messageId}-b${index}`} />
        ))}
      </div>
    </EmojiIndexContext>
  )
}
