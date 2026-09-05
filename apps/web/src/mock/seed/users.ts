import type { Role, User } from '../types'

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
    roleIds: ['r_core', 'r_maintainer'],
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
    roleIds: ['r_core'],
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
    roleIds: ['r_maintainer'],
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
    roleIds: ['r_maintainer'],
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
    roleIds: [],
  },
]

export const roles: Role[] = [
  { id: 'r_core', name: 'Core Team', color: '#9b59b6', position: 0 },
  { id: 'r_maintainer', name: 'Maintainer', color: '#3498db', position: 1 },
  { id: 'r_helper', name: 'Helper', color: '#1abc9c', position: 2 },
]
