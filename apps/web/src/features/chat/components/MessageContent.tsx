import { renderMarkdownBlocks } from '../markdown'
import type { MentionToken } from '../chatLib'

export function MessageContent({ content, mentionTokens }: { content: string; mentionTokens: MentionToken[] }) {
  if (!content.trim()) return null
  return (
    <div className="text-sm leading-5 font-medium whitespace-pre-wrap text-foreground/90 [overflow-wrap:anywhere] max-[899px]:text-xs max-[899px]:leading-[17px]">
      {renderMarkdownBlocks(content, 'message', mentionTokens)}
    </div>
  )
}
