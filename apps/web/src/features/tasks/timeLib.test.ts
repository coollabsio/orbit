import { describe, expect, test } from 'bun:test'
import { takeSpend, todayLocalDate, workedOnLabel } from './timeLib'

test('gives the local date, not the UTC one', () => {
  // 23:30 local time on Nov 24: the date stays Nov 24 whatever the UTC date is
  expect(todayLocalDate(new Date(2026, 10, 24, 23, 30))).toBe('2026-11-24')
  expect(todayLocalDate(new Date(2026, 0, 5, 0, 10))).toBe('2026-01-05')
})

test('labels a work day on its local calendar day', () => {
  const now = new Date(2026, 10, 24)
  expect(workedOnLabel('2026-11-10', now)).toBe(new Date(2026, 10, 10).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }))
  expect(workedOnLabel('2025-12-31', now)).toContain('2025')
})

describe('takeSpend', () => {
  const line = (text: string) => ({ type: 'paragraph', props: {}, content: [{ type: 'text', text, styles: {} }], children: [] })

  test('takes the /spend line out and keeps the rest', () => {
    const rest = line('Fixed the retry')
    expect(takeSpend([rest, line('/spend 1h 30m')])).toEqual({ blocks: [rest], spend: { minutes: 90 } })
    expect(takeSpend([line('/SPEND 2h 2026-11-10')])).toEqual({ blocks: [], spend: { minutes: 120, workedOn: '2026-11-10' } })
  })

  test('leaves a comment without a /spend line alone', () => {
    const blocks = [line('spend 1h on it'), line('see /spend 1h')]
    expect(takeSpend(blocks)).toEqual({ blocks, spend: null })
  })

  test('takes a /spend line out of a paragraph with line breaks (Shift+Enter)', () => {
    const block = { type: 'paragraph', props: {}, content: [{ type: 'text', text: 'Fixed ', styles: {} }, { type: 'text', text: 'the retry\n/spend 1h 30m\nand tests', styles: { bold: true } }], children: [] }
    expect(takeSpend([block])).toEqual({
      blocks: [{ ...block, content: [{ type: 'text', text: 'Fixed ', styles: {} }, { type: 'text', text: 'the retry\nand tests', styles: { bold: true } }] }],
      spend: { minutes: 90 },
    })
    expect(takeSpend([line('/spend 2h\nDone')])).toEqual({ blocks: [line('Done')], spend: { minutes: 120 } })
    expect(takeSpend([line('Done\n/spend 2h')])).toEqual({ blocks: [line('Done')], spend: { minutes: 120 } })
  })

  test('leaves a /spend line in a code block alone', () => {
    const blocks = [{ ...line('/spend 1h'), type: 'codeBlock' }]
    expect(takeSpend(blocks)).toEqual({ blocks, spend: null })
  })

  test('refuses a /spend line it cannot read', () => {
    expect(() => takeSpend([line('/spend 3d')])).toThrow('Could not read')
    expect(() => takeSpend([line('/spend 25h')])).toThrow('Could not read')
  })
})
