import type { Project, User } from '../types'

// Coolify team as mock users
export const users: User[] = [
  {
    id: 'u_shadow',
    name: 'ShadowArcanist',
    handle: 'shadowarcanist',
    email: 'shadowarcanist@coolify.io',
    role: 'Owner',
    color: '#8b5cf6',
    online: true,
    title: 'Community Lead',
  },
  {
    id: 'u_andras',
    name: 'Andras',
    handle: 'andras',
    email: 'andras@coolify.io',
    role: 'Admin',
    color: '#0ea5e9',
    online: true,
    title: 'Founder',
  },
  {
    id: 'u_adiology',
    name: 'Adiology',
    handle: 'adiology',
    email: 'adiology@coolify.io',
    role: 'Member',
    color: '#f59e0b',
    online: false,
    title: 'Backend Engineer',
  },
  {
    id: 'u_cinzya',
    name: 'Cinzya',
    handle: 'cinzya',
    email: 'cinzya@coolify.io',
    role: 'Member',
    color: '#10b981',
    online: true,
    title: 'DevOps',
  },
  {
    id: 'u_peak',
    name: 'Peak',
    handle: 'peak',
    email: 'peak@coolify.io',
    role: 'Member',
    color: '#ec4899',
    online: false,
    title: 'Frontend Engineer',
  },
]

export const projects: Project[] = [
  { id: 'p_infra', name: 'Infrastructure', key: 'INF', color: '#8b5cf6' },
  { id: 'p_web', name: 'Website', key: 'WEB', color: '#0ea5e9' },
  { id: 'p_internal', name: 'Internal Tools', key: 'INT', color: '#10b981' },
]
