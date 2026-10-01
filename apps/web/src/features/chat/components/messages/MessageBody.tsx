import { Fragment, useState } from 'react'
import { Attachments } from '@/components/common/Attachments'
import { Button } from '@/components/ui/button'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { Message } from '../../api/types'
import { useChatHost } from '../../chatHost'
import { decodeMentions } from '../../lib/mentionTokens'
import { extractLinkUrls, isLongMessage } from './messageText'
import { useChatPeople } from './people'

/** Cards for the Orbit task and page URLs in a message; other URLs give nothing. */
function LinkCards({ body }: { body: string }) {
  const { renderLinkCard } = useChatHost()
  const cards = extractLinkUrls(body).flatMap((url) => {
    const card = renderLinkCard(url)
    return card ? [{ url, card }] : []
  })
  if (cards.length === 0) return null
  return (
    <div data-slot="message-link-cards" className="mt-1.5 flex max-w-md flex-col gap-1.5">
      {cards.map(({ url, card }) => (
        <Fragment key={url}>{card}</Fragment>
      ))}
    </div>
  )
}

/**
 * What a message says: its markdown with mentions shown as names, "(edited)", attachments and link cards. A long
 * message collapses behind "Show more". No list or toolbar concerns, so previews (pins, search, unreads) can use it.
 */
export function MessageBody({ message }: { message: Message }) {
  const people = useChatPeople()
  const [expanded, setExpanded] = useState(false)

  if (message.deleted) {
    return (
      <p data-slot="message-body" data-deleted="" className="text-muted-foreground italic">
        This message was deleted
      </p>
    )
  }

  const text = decodeMentions(message.body, people.members, people.channels)
  const long = isLongMessage(text)

  return (
    <div data-slot="message-body" className="min-w-0">
      {text ? (
        <div
          data-slot="message-text"
          data-collapsed={long && !expanded ? '' : undefined}
          data-edited={message.editedAt ? '' : undefined}
          // Code scrolls sideways instead of wrapping; with "(edited)" the last paragraph is inline so the mark follows it.
          className="leading-[1.5] wrap-anywhere text-foreground/85 data-collapsed:max-h-[30em] data-collapsed:overflow-hidden data-edited:[&>p:nth-last-child(2)]:inline [&_pre]:overflow-x-auto [&_pre]:whitespace-pre [&_pre_code]:whitespace-pre"
        >
          {renderMarkdownBlocks(text, message.id, people.tokens)}
          {message.editedAt ? (
            <span data-slot="message-edited" className="ml-1 text-xs text-muted-foreground">
              (edited)
            </span>
          ) : null}
        </div>
      ) : null}
      {long ? (
        <Button variant="link" size="xs" className="mt-0.5 h-6 px-0" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? 'Show less' : 'Show more'}
        </Button>
      ) : null}
      <Attachments attachments={message.attachments} hasTextContent={text !== ''} />
      <LinkCards body={message.body} />
    </div>
  )
}
