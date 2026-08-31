import type { AppState } from './types'
import { projects, roles, users } from './seed/users'
import { statuses, tasks } from './seed/tasks'
import { docs } from './seed/docs'
import { mailFolders, mailThreads } from './seed/mail'
import { channels, chatCategories, chatMessages, directMessages } from './seed/chat'
import { notifications } from './seed/notifications'
import { webhooks } from './seed/webhooks'
import { customEmojis } from './seed/customEmojis'
import { sessions } from './seed/sessions'

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
    directMessages,
    chatMessages,
    webhooks,
    customEmojis,
    sessions,
    typingUsers: {},
    notifications,
  }
}
