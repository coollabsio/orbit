import type { AppState } from './types'
import { users } from '@/mock/seed/users'
import { channels } from '@/mock/seed/channels'
import { notifications } from '@/mock/seed/notifications'

export function seedState(): AppState {
  return {
    currentUserId: 'u_shadow',
    users,
    channels,
    notifications,
  }
}
