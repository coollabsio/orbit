import { expect, test } from 'bun:test'
import { fullDate } from '@/lib/format'
import { newWorkspaceTrialNote } from './billing'

const now = Date.parse('2026-11-11T12:00:00Z')

test('a new workspace starts the trial when the account has had none', () => {
  expect(newWorkspaceTrialNote({ days: 14, ends_at: null }, now)).toBe('This workspace starts your 14-day free trial. Later workspaces share it.')
  expect(newWorkspaceTrialNote({ days: 0, ends_at: null }, now)).toBe('This workspace needs a plan before you can add to it.')
})

test('a new workspace shares a running trial and is read-only after it', () => {
  const later = '2026-11-20T12:00:00Z'
  const earlier = '2026-11-01T12:00:00Z'
  expect(newWorkspaceTrialNote({ days: 14, ends_at: later }, now)).toBe(`This workspace shares your free trial, which ends on ${fullDate(later)}.`)
  expect(newWorkspaceTrialNote({ days: 14, ends_at: earlier }, now)).toBe(`Your free trial ended on ${fullDate(earlier)}. This workspace stays read-only until it has a plan.`)
})

test('nothing is said without billing', () => {
  expect(newWorkspaceTrialNote(undefined, now)).toBeNull()
})
