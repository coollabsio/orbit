import { expect, test } from 'bun:test'
import type { MailThreadSummary } from '@orbit/api/generated/types.gen'
import { dateGroup, foldedRange, groupThreads, senderColor, splitTextQuote } from './mailLib'

const now = new Date(2026, 10, 18, 15, 0) // Wed 18 Nov 2026, 15:00 local
const at = (days: number, hour = 9) => new Date(2026, 10, 18 - days, hour).toISOString()

test('date groups by the local day', () => {
  expect(dateGroup(at(0, 0), now)).toBe('Today')
  expect(dateGroup(at(1, 23), now)).toBe('Yesterday')
  expect(dateGroup(at(6), now)).toBe('This week')
  expect(dateGroup(new Date(2026, 9, 3).toISOString(), now)).toBe(new Date(2026, 9, 3).toLocaleDateString(undefined, { month: 'long' }))
  expect(dateGroup(new Date(2025, 9, 3).toISOString(), now)).toBe(new Date(2025, 9, 3).toLocaleDateString(undefined, { month: 'long', year: 'numeric' }))
  // a time later today (clock skew) is still today
  expect(dateGroup(new Date(2026, 10, 18, 23).toISOString(), now)).toBe('Today')
})

test('a header before each new group, none twice', () => {
  const thread = (id: string, iso: string) => ({ id, last_message_at: iso, last_sent_at: null }) as unknown as MailThreadSummary
  const entries = groupThreads([thread('a', at(0)), thread('b', at(0, 8)), thread('c', at(1)), thread('d', at(3))], false, now)
  expect(entries.map((entry) => (entry.kind === 'group' ? `#${entry.label}` : entry.thread.id))).toEqual(['#Today', 'a', 'b', '#Yesterday', 'c', '#This week', 'd'])
  expect(groupThreads([], false, now)).toEqual([])
})

test('the Sent list groups by the time sent', () => {
  const sent = { id: 's', last_message_at: at(0), last_sent_at: at(2) } as unknown as MailThreadSummary
  expect(groupThreads([sent], true, now)[0]).toEqual({ kind: 'group', label: 'This week' })
})

test('a sender keeps one colour, whatever the case', () => {
  expect(senderColor('Ann@Example.com')).toBe(senderColor('ann@example.com'))
  expect(senderColor('ann@example.com')).not.toBe(senderColor('bob@example.com'))
  expect(senderColor(null)).toMatch(/^oklch\(/)
})

test('the trailing quote of a plain-text reply folds with its "wrote:" line', () => {
  const text = 'Thanks, done.\n\nOn Tue, Ann wrote:\n> Can you check?\n>\n> > older\n\n'
  expect(splitTextQuote(text)).toEqual({ body: 'Thanks, done.', quote: 'On Tue, Ann wrote:\n> Can you check?\n>\n> > older' })
})

test('no fold without a trailing quote, or when only the quote is there', () => {
  expect(splitTextQuote('Hello\n> quoted in the middle\nend').quote).toBeNull()
  expect(splitTextQuote('> only a quote\n> more').quote).toBeNull()
  expect(splitTextQuote('On Tue, Ann wrote:\n> only a quote').quote).toBeNull()
  // a long line before the quote is text, not an intro
  const long = `Body\n${'x'.repeat(210)}:\n> q`
  expect(splitTextQuote(long)).toEqual({ body: `Body\n${'x'.repeat(210)}:`, quote: '> q' })
})

test('threads of 5 or more fold their middle messages', () => {
  expect(foldedRange(4)).toBeNull()
  expect(foldedRange(5)).toEqual([1, 3])
  expect(foldedRange(7)).toEqual([1, 5])
})
