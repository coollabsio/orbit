import { updateState } from './store'
import type { User } from './types'

const now = () => new Date().toISOString()

/* ---------- notifications ---------- */

export function markNotificationRead(notificationId: string) {
  updateState((s) => ({
    ...s,
    notifications: s.notifications.map((n) =>
      n.id === notificationId ? { ...n, readAt: n.readAt ?? now() } : n,
    ),
  }))
}

export function markAllNotificationsRead() {
  updateState((s) => ({
    ...s,
    notifications: s.notifications.map((n) => ({ ...n, readAt: n.readAt ?? now() })),
  }))
}

/* ---------- user profile ---------- */

export function updateUserProfile(userId: string, patch: Partial<Pick<User, 'name' | 'email' | 'title'>>) {
  updateState((s) => ({
    ...s,
    users: s.users.map((u) => (u.id === userId ? { ...u, ...patch } : u)),
  }))
}
