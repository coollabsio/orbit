import { useContext } from 'react'
import { MarkdownTextContext } from '@/lib/markdownTextContext'
import { EmojiText } from './Emoji'

/** A run of plain text in rendered markdown: its emoji as Twemoji images, unless the host draws it (`MarkdownTextContext`). */
export function MarkdownText({ text }: { text: string }) {
  const render = useContext(MarkdownTextContext)
  return render ? render(text, (part) => <EmojiText text={part} />) : <EmojiText text={text} />
}
