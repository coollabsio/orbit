// Marketing seed (swapped in for apps/web/src/mock/seed/users.ts by vite.capture.config.ts).
// Same ids as the app seed so notifications/webhooks keep resolving; the Orbit team instead of placeholders.
import type { Role, User } from '@/mock/types'

export const users: User[] = [
  {
    id: 'u_shadow',
    name: 'Maya Chen',
    handle: 'maya',
    email: 'maya@orbit.dev',
    role: 'Owner',
    color: '#8b5cf6',
    online: true,
    title: 'Product Lead',
    roleIds: ['r_core'],
  },
  {
    id: 'u_andras',
    name: 'Leo Martins',
    handle: 'leo',
    email: 'leo@orbit.dev',
    role: 'Admin',
    color: '#0ea5e9',
    online: true,
    title: 'Engineering Lead',
    roleIds: ['r_core', 'r_maintainer'],
  },
  {
    id: 'u_adiology',
    name: 'Priya Nair',
    handle: 'priya',
    email: 'priya@orbit.dev',
    role: 'Member',
    color: '#f59e0b',
    online: true,
    title: 'Backend Engineer',
    roleIds: ['r_maintainer'],
  },
  {
    id: 'u_cinzya',
    name: 'Tomás Rivera',
    handle: 'tomas',
    email: 'tomas@orbit.dev',
    role: 'Member',
    color: '#10b981',
    online: true,
    title: 'Infrastructure',
    roleIds: ['r_maintainer'],
  },
  {
    id: 'u_peak',
    name: 'Sofia Lindqvist',
    handle: 'sofia',
    email: 'sofia@orbit.dev',
    role: 'Member',
    color: '#ec4899',
    online: false,
    title: 'Product Designer',
    roleIds: ['r_helper'],
  },
  {
    id: 'u_noah',
    name: 'Noah Becker',
    handle: 'noah',
    email: 'noah@orbit.dev',
    role: 'Member',
    color: '#f97316',
    online: false,
    title: 'Frontend Engineer',
    roleIds: ['r_maintainer'],
  },
]

export const roles: Role[] = [
  { id: 'r_core', name: 'Founders', color: '#a78bfa', position: 0 },
  { id: 'r_maintainer', name: 'Engineering', color: '#38bdf8', position: 1 },
  { id: 'r_helper', name: 'Design', color: '#f472b6', position: 2 },
]
