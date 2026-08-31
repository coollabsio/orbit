import { ago } from '../../lib/format'
import type { Session } from '../types'

export const sessions: Session[] = [
  { id: 's_1', device: 'MacBook Pro', browser: 'Firefox 143', location: 'Chennai, IN', ip: '103.48.12.7', lastActiveAt: ago(1, 'm'), current: true },
  { id: 's_2', device: 'iPhone 16', browser: 'Safari', location: 'Chennai, IN', ip: '103.48.12.7', lastActiveAt: ago(3, 'h'), current: false },
  { id: 's_3', device: 'Windows Desktop', browser: 'Chrome 141', location: 'Frankfurt, DE', ip: '188.34.9.101', lastActiveAt: ago(6, 'd'), current: false },
]
