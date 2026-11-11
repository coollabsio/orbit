import type { SearchInput } from './client'

/**
 * Deliberately outside the `['workspace', id]` prefix (like `notionImports` in `@/api/queryKeys`): realtime events
 * invalidate that whole prefix, and that invalidation pauses while an input has focus, which in chat is almost always.
 * Chat is kept current by `applyChatEvent` instead.
 */
export const chatKeys = {
  all: (workspaceId: string) => ['chat', workspaceId] as const,
  conversations: (workspaceId: string) => ['chat', workspaceId, 'conversations'] as const,
  categories: (workspaceId: string) => ['chat', workspaceId, 'categories'] as const,
  states: (workspaceId: string) => ['chat', workspaceId, 'states'] as const,
  /** Prefix of every message list of one conversation (the newest window and each `around` window). */
  messagesOf: (workspaceId: string, conversationId: string) => ['chat', workspaceId, 'messages', conversationId] as const,
  messages: (workspaceId: string, conversationId: string, around: string | null = null) =>
    ['chat', workspaceId, 'messages', conversationId, { around }] as const,
  /** Prefix of every thread query, for events that do not name the root. */
  threadPages: (workspaceId: string) => ['chat', workspaceId, 'thread'] as const,
  thread: (workspaceId: string, rootId: string) => ['chat', workspaceId, 'thread', rootId] as const,
  threads: (workspaceId: string, conversationId: string) => ['chat', workspaceId, 'threads', conversationId] as const,
  followedThreads: (workspaceId: string) => ['chat', workspaceId, 'followed-threads'] as const,
  unreadThreads: (workspaceId: string) => ['chat', workspaceId, 'unread-threads'] as const,
  pins: (workspaceId: string, conversationId: string) => ['chat', workspaceId, 'pins', conversationId] as const,
  files: (workspaceId: string, conversationId: string) => ['chat', workspaceId, 'files', conversationId] as const,
  searches: (workspaceId: string) => ['chat', workspaceId, 'search'] as const,
  search: (workspaceId: string, input: Omit<SearchInput, 'cursor'>) => ['chat', workspaceId, 'search', input] as const,
  /** Outside the `all` prefix: a resync must not read every page again. */
  linkPreview: (workspaceId: string, url: string) => ['chat-link-preview', workspaceId, url] as const,
  /** Outside the `all` prefix: not kept on the device, and read again each time the settings open. */
  webhooks: (workspaceId: string, conversationId: string) => ['chat-webhooks', workspaceId, conversationId] as const,
  /** Outside the `all` prefix: never kept on the device, and not read again by a resync. */
  deliveries: (workspaceId: string, webhookId: string) => ['chat-webhook-deliveries', workspaceId, webhookId] as const,
  /** Integration replies only this user sees, for one conversation. Client-only: they are gone after a reload. */
  ephemeral: (workspaceId: string, conversationId: string) => ['chat-ephemeral', workspaceId, conversationId] as const,
  /** Outside the `all` prefix: a resync must not touch the messages that only this tab has. */
  outbox: (workspaceId: string) => ['chat-outbox', workspaceId] as const,
}
