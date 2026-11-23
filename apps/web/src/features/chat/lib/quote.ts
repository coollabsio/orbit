import type { Message, ReplyQuote } from '../api/types'

/** The server quotes this many characters of a message. */
const QUOTE_LENGTH = 200

/**
 * What a message shows besides its text, for a preview without text: a sticker of the workspace or from KLIPY, or a
 * GIF from KLIPY. A row cached before media existed has no `media`.
 */
export function wordlessContentOf(message: Pick<Message, 'stickerId' | 'media'>): { sticker: boolean; gif: boolean } {
  return { sticker: message.stickerId != null || message.media?.kind === 'sticker', gif: message.media?.kind === 'gif' }
}

/** The quote that a reply to `message` carries, cut the way the server cuts it. */
export function quoteOf(message: Message): ReplyQuote {
  return {
    id: message.id,
    authorId: message.authorId,
    body: Array.from(message.body).slice(0, QUOTE_LENGTH).join(''),
    ...wordlessContentOf(message),
    botName: message.bot?.name ?? null,
  }
}
