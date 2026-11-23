import { useState } from 'react'
import type { MessageMedia } from '../../api/types'
import { mediaSize } from './messageText'

/**
 * A GIF or sticker from KLIPY, straight from KLIPY's CDN (no referrer goes with it). Its box is there before it loads,
 * so the list does not jump: a GIF at its aspect ratio, at most 300px either way, and narrower on a narrow screen; a
 * sticker in the square of a workspace sticker. An image that does not load leaves a short note.
 */
export function MessageMediaImage({ media }: { media: MessageMedia }) {
  const [failed, setFailed] = useState(false)
  const gif = media.kind === 'gif'
  if (failed) {
    return (
      <p data-slot="message-media" data-failed="" className="mt-1 text-xs text-muted-foreground first:mt-0">
        {gif ? 'GIF could not load' : 'Sticker could not load'}
      </p>
    )
  }
  const size = mediaSize(media)
  return (
    <img
      data-slot="message-media"
      data-kind={media.kind}
      src={media.url}
      alt={media.title || (gif ? 'GIF' : 'Sticker')}
      title={media.title || undefined}
      width={size.width}
      height={size.height}
      loading="lazy"
      decoding="async"
      draggable={false}
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
      // the width and height attributes give the box its aspect ratio, so `h-auto` keeps its shape when `max-w-full` narrows it
      className="mt-1 block first:mt-0 data-[kind=gif]:h-auto data-[kind=gif]:max-w-full data-[kind=gif]:rounded-md data-[kind=gif]:bg-muted data-[kind=sticker]:size-40 data-[kind=sticker]:object-contain data-[kind=sticker]:object-left"
    />
  )
}
