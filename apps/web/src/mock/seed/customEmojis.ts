import { ago } from '../../lib/format'
import type { CustomEmoji } from '../types'

const svg = (body: string, bg: string) =>
  `data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='64' height='64'%3E%3Crect width='64' height='64' rx='14' fill='%23${bg}'/%3E%3Ctext x='32' y='45' font-size='34' font-family='sans-serif' font-weight='700' text-anchor='middle' fill='%23fff'%3E${body}%3C/text%3E%3C/svg%3E`

export const customEmojis: CustomEmoji[] = [
  { id: 'ce_coolify', name: 'coolify', url: svg('C', 'a78bfa'), createdBy: 'u_shadow', createdAt: ago(10, 'd') },
  { id: 'ce_shipit', name: 'shipit', url: svg('GO', '22c55e'), createdBy: 'u_andras', createdAt: ago(6, 'd') },
]
