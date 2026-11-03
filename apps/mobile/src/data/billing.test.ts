import { expect, test } from 'bun:test'
import type { BillingView } from '@orbit/api/generated/types.gen'

import { billingBanner } from './billing'

const NOW = Date.parse('2026-10-27T12:00:00Z')
const view = (fields: Partial<BillingView>): BillingView => ({
  access: 'full',
  can_manage: true,
  cancel_at_period_end: false,
  exempt: false,
  has_customer: false,
  plans: [],
  trial_days: 14,
  ...fields,
})
const trial = (ends_at: string): BillingView['trial'] => ({ ends_at, members: { used: 1, limit: 3 }, pages: { used: 0, limit: 50 }, tasks: { used: 0, limit: 100 } })

test('a trial shows its days left, rounded up and at least one', () => {
  expect(billingBanner(view({ access: 'trial', trial: trial('2026-11-03T12:00:00Z') }), NOW)).toEqual({ kind: 'trial', text: 'Trial: 7 days left' })
  expect(billingBanner(view({ access: 'trial', trial: trial('2026-10-27T13:00:00Z') }), NOW)?.text).toBe('Trial: 1 day left')
  expect(billingBanner(view({ access: 'trial', trial: trial('2026-10-27T11:00:00Z') }), NOW)?.text).toBe('Trial: 1 day left')
})

test('read-only shows the lock line with no billing action for any member', () => {
  expect(billingBanner(view({ access: 'read_only', can_manage: true }), NOW)).toEqual({ kind: 'read_only', text: 'This workspace is read-only.' })
  expect(billingBanner(view({ access: 'read_only', can_manage: false }), NOW)).toEqual({ kind: 'read_only', text: 'This workspace is read-only.' })
})

test('full access, a trial without data and no billing show nothing', () => {
  expect(billingBanner(view({}), NOW)).toBeNull()
  expect(billingBanner(view({ access: 'trial', trial: null }), NOW)).toBeNull()
  expect(billingBanner(undefined, NOW)).toBeNull()
})
