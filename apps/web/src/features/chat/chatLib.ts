// Helpers copied from the chat reference (frontend/src/components/chat/MessageItem.tsx
// and frontend/src/lib/time.ts), adapted from JSON content to plain text.
import { isMentionBoundary } from '@/lib/mentions'
import type { AppState, ChatMessage, Role, User } from '@/mock/types'

/* ---------- time (the chat reference lib/time.ts) ---------- */

export function formatShortTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', hour12: true })
}

export function formatMessageDate(iso: string): string {
  return new Date(iso).toLocaleDateString([], { month: 'long', day: 'numeric', year: 'numeric' })
}

export function isSameDay(left: string, right: string): boolean {
  const l = new Date(left)
  const r = new Date(right)
  return (
    l.getFullYear() === r.getFullYear() && l.getMonth() === r.getMonth() && l.getDate() === r.getDate()
  )
}

/* ---------- authors ---------- */

export function displayName(state: AppState, message: ChatMessage): string {
  if (message.authorType === 'user') {
    return state.users.find((u) => u.id === message.authorId)?.name ?? 'Unknown'
  }
  if (message.authorType === 'webhook') return message.webhookName || 'Webhook'
  return message.externalAuthor?.name ?? message.authorType
}

export function authorUser(state: AppState, message: ChatMessage): User | undefined {
  return message.authorType === 'user'
    ? state.users.find((u) => u.id === message.authorId)
    : undefined
}

/* ---------- roles: members use the color of the highest role they have ---------- */

export function primaryRole(state: AppState, userId: string): Role | undefined {
  const user = state.users.find((u) => u.id === userId)
  if (!user || user.roleIds.length === 0) return undefined
  return [...state.roles].sort((a, b) => a.position - b.position).find((role) => user.roleIds.includes(role.id))
}

export function roleColor(state: AppState, userId: string): string | undefined {
  return primaryRole(state, userId)?.color
}

/** Name color for a message author: the highest role color, none for external senders. */
export function authorColor(state: AppState, message: ChatMessage): string | undefined {
  return message.authorType === 'user' ? roleColor(state, message.authorId) : undefined
}

/* ---------- mentions (generic mention helpers live in @/lib/mentions) ---------- */

export function messageMentionsCurrentUser(message: ChatMessage, me: User | undefined): boolean {
  if (!me) return false
  const content = message.content.toLowerCase()
  if (content.includes('@everyone') || content.includes('@here')) return true
  return (
    mentionsToken(content, me.handle.toLowerCase()) || mentionsToken(content, me.name.toLowerCase())
  )
}

function mentionsToken(content: string, label: string): boolean {
  const needle = `@${label}`
  let index = content.indexOf(needle)
  while (index !== -1) {
    if (isMentionBoundary(content[index + needle.length])) return true
    index = content.indexOf(needle, index + 1)
  }
  return false
}

/* ---------- reply jump (the chat reference ReplyReference.scrollToReply) ---------- */

export function jumpToMessage(messageId: string): void {
  const target = document.getElementById(`message-${messageId}`)
  if (!target) return
  target.scrollIntoView({ block: 'center', behavior: 'smooth' })
  target.classList.remove('reply-jump-highlight')
  void target.getBoundingClientRect()
  target.classList.add('reply-jump-highlight')
  window.setTimeout(() => target.classList.remove('reply-jump-highlight'), 1800)
}
