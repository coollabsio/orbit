import { renderMarkdownBlocks } from '../markdown'
import type { MentionToken } from '../chatLib'

export function MessageContent({ content, mentionTokens }: { content: string; mentionTokens: MentionToken[] }) {
  if (!content.trim()) return null
  return <div className="fc-msg-text">{renderMarkdownBlocks(content, 'message', mentionTokens)}</div>
}
