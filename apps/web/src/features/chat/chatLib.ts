import type { AppState, ChatMessage, User } from '../../mock/types'

/** The chat reference-style 12h time: "3:42 PM". */
export function chatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', hour12: true })
}

export function authorKey(message: ChatMessage): string {
  return message.authorType === 'user'
    ? `user:${message.authorId}`
    : `${message.authorType}:${message.externalAuthor?.name ?? message.authorId}`
}

export function displayName(state: AppState, message: ChatMessage): string {
  if (message.authorType === 'user') {
    return state.users.find((u) => u.id === message.authorId)?.name ?? 'Unknown'
  }
  return message.externalAuthor?.name ?? message.authorType
}

/** True when the message mentions the given user (@handle or @everyone/@here). */
export function mentionsUser(message: ChatMessage, user: User | undefined): boolean {
  if (!user) return false
  const content = message.content.toLowerCase()
  return (
    content.includes(`@${user.handle.toLowerCase()}`) ||
    content.includes('@everyone') ||
    content.includes('@here')
  )
}

/** Split content into plain text and @mention tokens for inline highlighting. */
export function splitMentions(content: string, users: User[]): Array<{ text: string; mention: boolean }> {
  const handles = users.map((u) => u.handle.toLowerCase())
  const parts: Array<{ text: string; mention: boolean }> = []
  const regex = /@[a-z0-9_]+/gi
  let last = 0
  for (const match of content.matchAll(regex)) {
    const token = match[0]
    const isKnown =
      handles.includes(token.slice(1).toLowerCase()) || token === '@everyone' || token === '@here'
    if (!isKnown) continue
    if (match.index > last) parts.push({ text: content.slice(last, match.index), mention: false })
    parts.push({ text: token, mention: true })
    last = match.index + token.length
  }
  if (last < content.length) parts.push({ text: content.slice(last), mention: false })
  return parts.length > 0 ? parts : [{ text: content, mention: false }]
}

/** Scroll a message into view and flash it, The chat reference's reply-jump behavior. */
export function jumpToMessage(messageId: string): void {
  const el = document.getElementById(`message-${messageId}`)
  if (!el) return
  el.scrollIntoView({ block: 'center', behavior: 'smooth' })
  el.classList.remove('chat-jump-highlight')
  // restart the animation on repeat jumps
  void el.offsetWidth
  el.classList.add('chat-jump-highlight')
  window.setTimeout(() => el.classList.remove('chat-jump-highlight'), 1900)
}
