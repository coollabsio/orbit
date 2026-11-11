import { describe, expect, it } from 'bun:test'
import { shouldShowDonationReminder } from './DonationReminder'

const at = (iso: string) => new Date(iso)

describe('shouldShowDonationReminder', () => {
  it('shows when never dismissed or the value is broken', () => {
    expect(shouldShowDonationReminder(null, at('2026-11-10T12:00:00'))).toBe(true)
    expect(shouldShowDonationReminder('soon', at('2026-11-10T12:00:00'))).toBe(true)
  })

  it('stays hidden for the rest of the month it was dismissed in', () => {
    const dismissed = String(at('2026-11-02T09:00:00').getTime())
    expect(shouldShowDonationReminder(dismissed, at('2026-11-30T23:00:00'))).toBe(false)
  })

  it('comes back in the next month, also across a year', () => {
    expect(shouldShowDonationReminder(String(at('2026-11-30T23:00:00').getTime()), at('2026-12-01T08:00:00'))).toBe(true)
    expect(shouldShowDonationReminder(String(at('2026-12-15T10:00:00').getTime()), at('2027-12-20T10:00:00'))).toBe(true)
  })
})
