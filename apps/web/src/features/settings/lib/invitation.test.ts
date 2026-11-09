import { expect, test } from 'bun:test'
import { invitationRequest } from './invitation'

test('a guest invitation names its channels and needs one or more', () => {
  expect(invitationRequest('g@example.com', 'Guest', [], 'manual')).toEqual({ problem: 'Choose one or more channels for the guest.' })
  expect(invitationRequest('g@example.com', 'Guest', ['c1', 'c2'], 'smtp')).toEqual({
    body: { email: 'g@example.com', role: 'guest', delivery: 'smtp', conversation_ids: ['c1', 'c2'] },
  })
})

test('other roles send no channels, even when some were chosen before', () => {
  expect(invitationRequest('m@example.com', 'Member', ['c1'], 'manual')).toEqual({ body: { email: 'm@example.com', role: 'member', delivery: 'manual' } })
  expect(invitationRequest('a@example.com', 'Admin', [], 'manual')).toEqual({ body: { email: 'a@example.com', role: 'admin', delivery: 'manual' } })
})
