import type { AppState } from './types'
import { roles, users } from '@/mock/seed/users'
import { docs } from '@/mock/seed/docs'
import { mailFolders, mailThreads } from '@/mock/seed/mail'
import { channels, chatCategories, chatMessages, directMessages } from '@/mock/seed/chat'
import { notifications } from '@/mock/seed/notifications'
import { webhooks } from '@/mock/seed/webhooks'
import { customEmojis } from '@/mock/seed/customEmojis'

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
