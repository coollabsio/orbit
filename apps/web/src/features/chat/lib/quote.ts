import type { Message, ReplyQuote } from '../api/types'

/** The server quotes this many characters of a message. */
const QUOTE_LENGTH = 200

/** The quote that a reply to `message` carries, cut the way the server cuts it. */
export function quoteOf(message: Message): ReplyQuote {
  return { id: message.id, authorId: message.authorId, body: Array.from(message.body).slice(0, QUOTE_LENGTH).join(''), sticker: message.stickerId != null }
}
