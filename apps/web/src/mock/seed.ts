import type { AppState } from './types'
import { projects, roles, users } from './seed/users'
import { statuses, tasks } from './seed/tasks'
import { docs } from './seed/docs'
import { mailFolders, mailThreads } from './seed/mail'
import { channels, chatCategories, chatMessages } from './seed/chat'
import { notifications } from './seed/notifications'
import { webhooks } from './seed/webhooks'
import { customEmojis } from './seed/customEmojis'

export function seedState(): AppState {
  return {
    currentUserId: 'u_shadow',
    workspace: { name: 'Orbit', iconUrl: null },
    users,
    roles,
    projects,
    statuses,
    tasks,
    docs,
    mailFolders,
    mailThreads,
    chatCategories,
    channels,
    chatMessages,
    webhooks,
    customEmojis,
    typingUsers: {},
    notifications,
  }
}
