import { ago } from '../../lib/format'
import type { Session } from '../types'

export const sessions: Session[] = [
  { id: 's_1', userId: 'u_shadow', device: 'MacBook Pro', browser: 'Firefox 143', lastActiveAt: ago(1, 'm'), current: true },
  { id: 's_2', userId: 'u_shadow', device: 'iPhone 16', browser: 'Safari', lastActiveAt: ago(3, 'h'), current: false },
  { id: 's_3', userId: 'u_andras', device: 'Linux Desktop', browser: 'Chrome 141', lastActiveAt: ago(20, 'm'), current: false },
  { id: 's_4', userId: 'u_cinzya', device: 'MacBook Air', browser: 'Safari', lastActiveAt: ago(2, 'h'), current: false },
  { id: 's_5', userId: 'u_peak', device: 'Windows Desktop', browser: 'Edge', lastActiveAt: ago(4, 'd'), current: false },
]
