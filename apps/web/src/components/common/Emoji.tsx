import { useState } from 'react'
import { cn } from 'cn'
import { splitEmoji, twemojiUrl } from '@/lib/twemoji'

/**
 * One emoji as its Twemoji image, sized by the text around it (1.25em). If the image does not exist (an emoji newer
 * than the bundled set), the character shows in the system font.
 */
function EmojiImage({ emoji, url, className }: { emoji: string; url: string; className?: string }) {
  const [failed, setFailed] = useState(false)
  if (failed) return <>{emoji}</>
  return (
    <img
      data-slot="emoji"
      src={url}
      alt={emoji}
      draggable={false}
      decoding="async"
      className={cn('inline-block size-[1.25em] align-[-0.25em] select-none', className)}
      onError={() => setFailed(true)}
    />
  )
}

/** Renders an emoji value (a reaction, a page, teamspace or callout icon). A value that is not one emoji shows as text. */
export function Emoji({ value, className }: { value: string; className?: string }) {
  const url = twemojiUrl(value)
  return url ? <EmojiImage emoji={value} url={url} className={className} /> : <>{value}</>
}

/** Text with its emoji as Twemoji images. */
export function EmojiText({ text }: { text: string }) {
  return (
    <>
      {splitEmoji(text).map((segment, index) =>
        segment.url ? <EmojiImage key={index} emoji={segment.text} url={segment.url} /> : segment.text,
      )}
    </>
  )
}
