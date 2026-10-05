import { useContext, useState } from 'react'
import { cn } from 'cn'
import { customEmojiName } from '@/lib/customEmoji'
import { CustomEmojiContext } from '@/lib/customEmojiContext'
import { splitEmoji, twemojiUrl } from '@/lib/twemoji'

/**
 * One emoji as its Twemoji image, sized by the text around it (1.25em). If the image does not exist (an emoji newer
 * than the bundled set), the character shows in the system font.
 */
function EmojiImage({ emoji, url, title, className }: { emoji: string; url: string; title?: string; className?: string }) {
  const [failed, setFailed] = useState(false)
  if (failed) return <>{emoji}</>
  return (
    <img
      data-slot="emoji"
      src={url}
      alt={emoji}
      title={title}
      draggable={false}
      decoding="async"
      className={cn('inline-block size-[1.25em] align-[-0.25em] select-none', className)}
      onError={() => setFailed(true)}
    />
  )
}

/**
 * Renders an emoji value (a reaction, a page, teamspace or callout icon). `:name:` of a custom emoji of the workspace
 * (`CustomEmojiContext`) shows its image, at the size and place of a Twemoji image; a copy of the text gets `:name:`
 * back. A value that is not one emoji shows as text.
 */
export function Emoji({ value, className }: { value: string; className?: string }) {
  const custom = useContext(CustomEmojiContext).get(customEmojiName(value) ?? '')
  if (custom) return <EmojiImage emoji={value} url={custom.url} title={value} className={cn('object-contain', className)} />
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
