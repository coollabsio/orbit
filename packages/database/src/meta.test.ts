import { expect, test } from 'bun:test'
import { ruleSummary, ruleValueText } from './meta'
import { ctx, property } from './testing'

test('chip and value texts of rules', () => {
  const cases = [
    [{ property_id: 'p-text', operator: 'contains', value: 'abc' }, 'p-text', 'contains “abc”', 'abc'],
    [{ property_id: 'p-sel', operator: 'is', value: ['o-a', 'o-b'] }, 'p-sel', 'Zeta, Alpha', 'Zeta, Alpha'],
    [{ property_id: 'p-person', operator: 'not_contains', value: ['me'] }, 'p-person', 'does not contain Me', 'Me'],
    [{ property_id: 'p-date', operator: 'before', value: 'one_week_ago' }, 'p-date', 'is before One week ago', 'One week ago'],
    [{ property_id: 'p-date', operator: 'is', value: '2026-11-09' }, 'p-date', 'is 2026-11-09', '2026-11-09'],
    [{ property_id: 'p-num', operator: '>', value: 3 }, 'p-num', '> 3', '3'],
    [{ property_id: 'p-check', operator: 'is', value: false }, 'p-check', 'Unchecked', 'Unchecked'],
    [{ property_id: 'p-text', operator: 'contains', value: '' }, 'p-text', 'All', null],
    [{ property_id: 'p-text', operator: 'is_empty' }, 'p-text', 'is empty', null],
  ] as const
  for (const [rule, id, summary, value] of cases) {
    expect([ruleSummary(rule, property(id), ctx), ruleValueText(rule, property(id), ctx)]).toEqual([summary, value])
  }
})
