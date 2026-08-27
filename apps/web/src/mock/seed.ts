import type { AppState } from './types'
import { projects, users } from './seed/users'
import { tasks } from './seed/tasks'
import { docs } from './seed/docs'
import { mailFolders, mailThreads } from './seed/mail'
import { channels, chatCategories, chatMessages } from './seed/chat'
import { notifications } from './seed/notifications'
import { webhooks } from './seed/webhooks'

export function seedState(): AppState {
  return {
    currentUserId: 'u_shadow',
    users,
    projects,
    tasks,
    docs,
    mailFolders,
    mailThreads,
    chatCategories,
    channels,
    chatMessages,
    webhooks,
    typingUsers: {},
    notifications,
  }
}
