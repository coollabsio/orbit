import type { AppState } from './types'
import { projects, users } from './seed/users'
import { tasks } from './seed/tasks'
import { docs } from './seed/docs'
import { mailFolders, mailThreads } from './seed/mail'
import { channels, chatMessages } from './seed/chat'
import { notifications } from './seed/notifications'

export function seedState(): AppState {
  return {
    currentUserId: 'u_alice',
    users,
    projects,
    tasks,
    docs,
    mailFolders,
    mailThreads,
    channels,
    chatMessages,
    notifications,
  }
}
