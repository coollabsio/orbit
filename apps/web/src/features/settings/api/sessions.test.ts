import { expect, test } from 'bun:test'
import type { SessionRecord } from '@/api/generated/types.gen'
import { sessionView } from './sessions'

test('session records identify the browser session without mock device data', () => {
  const record = {
    id: 'session-1', created_at: '2026-09-04T10:00:00Z', last_activity_at: '2026-09-04T11:00:00Z',
    idle_expires_at: '2026-09-05T11:00:00Z', absolute_expires_at: '2026-10-04T10:00:00Z',
    user: { id: 'user-1', email: 'ada@orbit.test', display_name: 'Ada' },
    current: true,
  } as SessionRecord & { current: boolean }
  expect(sessionView(record)).toMatchObject({ id: 'session-1', current: true, device: 'Signed-in device', browser: 'Orbit web' })
})
