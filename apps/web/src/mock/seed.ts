import type { AppState } from './types'
import { roles, users } from './seed/users'
import { docs } from './seed/docs'
import { mailFolders, mailThreads } from './seed/mail'
import { channels, chatCategories, chatMessages, directMessages } from './seed/chat'
import { notifications } from './seed/notifications'
import { webhooks } from './seed/webhooks'
import { customEmojis } from './seed/customEmojis'

export function seedState(): AppState {
  return {
    currentUserId: 'u_shadow',
    users,
    roles,
    docs,
    mailFolders,
    mailThreads,
    chatCategories,
    channels,
    directMessages,
    chatMessages,
    webhooks,
    customEmojis,
    typingUsers: {},
    notifications,
  }
}
