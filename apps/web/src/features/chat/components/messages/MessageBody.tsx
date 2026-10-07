import { Fragment, useContext, useState, type ReactNode } from 'react'
import { Attachments } from '@/components/common/Attachments'
import { Button } from '@/components/ui/button'
import { CustomEmojiContext } from '@/lib/customEmojiContext'
import { InternalLinkContext } from '@/lib/internalLinkContext'
import { renderMarkdownBlocks } from '@/lib/markdown'
import { MarkdownTextContext } from '@/lib/markdownTextContext'
import type { Message } from '../../api/types'
import { useChatHost } from '../../chatHost'
import { decodeMentions } from '../../lib/mentionTokens'
import { LinkEmbed } from './LinkEmbed'
import { extractLinkUrls, isJumboEmoji, isLongMessage, standaloneLinkUrls } from './messageText'
import { useChatPeople } from './people'
import { findTypedTaskIds, linkTaskIds, taskIdUrl } from './taskIds'

/**
 * Cards for the URLs in a message: the host's card for an Orbit task, page or view, a preview for another site. In
 * `body` the task identifiers are task URLs already (`linkTaskIds`).
 */
function LinkCards({ body }: { body: string }) {
  const { renderLinkCard } = useChatHost()
  const urls = extractLinkUrls(body)
  if (urls.length === 0) return null
  return (
    // a card shows nothing while it loads and when its URL has nothing to show
    <div data-slot="message-link-cards" className="mt-1.5 flex max-w-md flex-col gap-1.5 empty:hidden">
      {urls.map((url) => (
        <Fragment key={url}>{renderLinkCard(url) ?? <LinkEmbed url={url} />}</Fragment>
      ))}
    </div>
  )
}

/**
 * What a message says: its markdown with mentions shown as names, "(edited)", its sticker, attachments and link cards. A long
 * message collapses behind "Show more". No list or toolbar concerns, so previews (pins, search, unreads) can use it.
 * An Orbit URL the reader can see shows as a chip; one that is a whole line shows only as its card. A typed task
 * identifier (`ENG-12`) shows as the URL of its task does, and stays the typed text when the reader cannot see the task
 * or a backslash is before it (`\ENG-12`). A message of only emoji shows them large.
 */
export function MessageBody({ message }: { message: Message }) {
  const people = useChatPeople()
  const { renderLink, taskKeys } = useChatHost()
  const customEmoji = useContext(CustomEmojiContext)
  const [expanded, setExpanded] = useState(false)
  const [stickerGone, setStickerGone] = useState(false)

  if (message.deleted) {
    return (
      <p data-slot="message-body" data-deleted="" className="text-muted-foreground italic">
        This message was deleted
      </p>
    )
  }

  const text = decodeMentions(message.body, people.members, people.channels)
  const origin = window.location.origin
  const linkBody = linkTaskIds(message.body, taskKeys, origin)
  const standalone = standaloneLinkUrls(linkBody)
  const long = isLongMessage(text)

  /**
   * A run of plain text of the message, with each task identifier in it as the chip of its task. An escaped one
   * (`\ENG-12`) stays text and loses its backslash.
   */
  function textRun(run: string, plain: (text: string) => ReactNode): ReactNode {
    const parts: ReactNode[] = []
    let cursor = 0
    for (const id of findTypedTaskIds(run, taskKeys)) {
      const url = taskIdUrl(id.identifier, origin)
      parts.push(
        <Fragment key={id.start}>
          {plain(run.slice(cursor, id.start))}
          {id.escaped ? null : renderLink(url, run.slice(id.start, id.end), standalone.includes(url))}
        </Fragment>,
      )
      cursor = id.escaped ? id.start + 1 : id.end
    }
    return parts.length === 0 ? plain(run) : [...parts, <Fragment key="rest">{plain(run.slice(cursor))}</Fragment>]
  }

  return (
    <div data-slot="message-body" className="min-w-0">
      {text ? (
        <div
          data-slot="message-text"
          data-collapsed={long && !expanded ? '' : undefined}
          data-edited={message.editedAt ? '' : undefined}
          data-jumbo={isJumboEmoji(text, customEmoji) ? '' : undefined}
          // Code scrolls sideways instead of wrapping; with "(edited)" the last paragraph is inline so the mark follows it.
          // A paragraph whose only content was a link that its card replaced is empty: it takes no line.
          // A message of only emoji shows them at 48px.
          className="leading-[1.5] wrap-anywhere text-foreground/85 data-collapsed:max-h-[30em] data-collapsed:overflow-hidden data-edited:[&>p:nth-last-child(2)]:inline data-jumbo:[&_[data-slot=emoji]]:size-12 [&_pre]:overflow-x-auto [&_pre]:whitespace-pre [&_pre_code]:whitespace-pre [&>p:empty]:hidden"
        >
          <InternalLinkContext value={(url, plain) => renderLink(url, plain, standalone.includes(url))}>
            <MarkdownTextContext value={textRun}>{renderMarkdownBlocks(text, message.id, people.tokens, { chat: true })}</MarkdownTextContext>
          </InternalLinkContext>
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
      {message.sticker && !stickerGone ? (
        <img
          data-slot="message-sticker"
          src={message.sticker.url}
          alt={message.sticker.name}
          title={message.sticker.name}
          draggable={false}
          loading="lazy"
          // deleted since this message was loaded
          onError={() => setStickerGone(true)}
          // the box is there before the image loads, so the list does not jump
          className="mt-1 size-40 object-contain object-left first:mt-0"
        />
      ) : message.stickerId ? (
        <p data-slot="message-sticker" data-deleted="" className="text-muted-foreground italic">
          Sticker was deleted
        </p>
      ) : null}
      <Attachments attachments={message.attachments} hasTextContent={text !== ''} />
      <LinkCards body={linkBody} />
    </div>
  )
}
