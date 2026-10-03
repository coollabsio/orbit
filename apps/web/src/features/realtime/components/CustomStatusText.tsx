import { Emoji, EmojiText } from '@/components/common/Emoji'

/** A custom status on one line: its emoji, then its text, with the same Twemoji images as chat messages. */
export function CustomStatusText({ emoji, text }: { emoji: string | null; text: string | null }) {
  return (
    <>
      {emoji ? <Emoji value={emoji} /> : null}
      {emoji && text ? ' ' : null}
      {text ? <EmojiText text={text} /> : null}
    </>
  )
}
