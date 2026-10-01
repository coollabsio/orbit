import type { AppState } from './types'
import { users } from '@/mock/seed/users'
import { mailFolders, mailThreads } from '@/mock/seed/mail'
import { channels } from '@/mock/seed/channels'
import { notifications } from '@/mock/seed/notifications'

export function seedState(): AppState {
  return {
    currentUserId: 'u_shadow',
    users,
    mailFolders,
    mailThreads,
    channels,
    notifications,
  }
}
