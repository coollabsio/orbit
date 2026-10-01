import type { Channel } from '@/mock/types'

// Rows of the Home dashboard's "Active channels" card. Chat itself has its own data layer (features/chat/api).
export const channels: Channel[] = [
  { id: 'c_general', name: 'general', description: 'Team-wide announcements and chatter', unreadCount: 0 },
  { id: 'c_design', name: 'design', description: 'UI, UX, and visual direction', unreadCount: 0 },
  { id: 'c_engineering', name: 'engineering', description: 'Backend, frontend, and everything between', unreadCount: 3 },
  { id: 'c_infra', name: 'infra', description: 'Servers, deploys, and incidents', unreadCount: 1 },
  { id: 'c_support', name: 'support', description: 'Forwarded customer conversations', unreadCount: 2 },
]
