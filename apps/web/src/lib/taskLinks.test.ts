import { expect, test } from 'bun:test'
import { isTaskUuid, normalizeTaskParam, parseTaskIdentifier, taskBranchName, taskParamMatches, taskPath, taskSlug } from './taskLinks'

const id = '0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0d'

test('a task link uses the identifier once the number and project key are known, else the id', () => {
  expect(taskPath({ id, number: 12, projectKey: 'ENG' })).toBe('/tasks/ENG-12')
  expect(taskPath({ id, number: 12, projectKey: 'ENG' }, '/views/view-1')).toBe('/views/view-1/ENG-12')
  expect(taskPath({ id })).toBe(`/tasks/${id}`)
  expect(taskSlug({ id, number: 12 })).toBe(id)
  expect(taskSlug({ id, number: 0, projectKey: 'ENG' })).toBe(id)
  expect(taskSlug({ id, number: null, projectKey: 'ENG' })).toBe(id)
})

test('identifiers parse in any case; ids, bare numbers and other text do not', () => {
  expect(parseTaskIdentifier('eng-12')).toEqual({ key: 'ENG', number: 12 })
  expect(parseTaskIdentifier('V2_APP-3')).toEqual({ key: 'V2_APP', number: 3 })
  for (const value of [id, '12', 'ENG-', 'ENG-0', 'ENG-012', '-12', 'EN G-1', 'ENG-1-2', 'TASK-91C0']) {
    expect([value, parseTaskIdentifier(value)]).toEqual([value, null])
  }
  expect(isTaskUuid(id)).toBe(true)
  expect(isTaskUuid(id.toUpperCase())).toBe(true)
  expect(isTaskUuid('ENG-12')).toBe(false)
})

test('a route segment names a task by id or by identifier in any case', () => {
  const task = { id, number: 12, projectKey: 'ENG' }
  expect(taskParamMatches(id, task)).toBe(true)
  expect(taskParamMatches('ENG-12', task)).toBe(true)
  expect(taskParamMatches('eng-12', task)).toBe(true)
  expect(taskParamMatches('ENG-13', task)).toBe(false)
  expect(taskParamMatches('OPS-12', task)).toBe(false)
  expect(taskParamMatches('ENG-12', { id })).toBe(false)
})

test('segments compare in one canonical form', () => {
  expect(normalizeTaskParam('eng-12')).toBe('ENG-12')
  expect(normalizeTaskParam(id.toUpperCase())).toBe(id)
})

test('a branch name is the identifier and a slug of the title', () => {
  expect(taskBranchName('ENG-12', 'Fix login')).toBe('eng-12-fix-login')
  // accents go, every run of other characters is one dash, and there is none at the ends
  expect(taskBranchName('ENG-12', '  Crème brûlée: 2nd “try” (v2)!  ')).toBe('eng-12-creme-brulee-2nd-try-v2')
  // cut at a word boundary to at most 50 characters
  const long = taskBranchName('ENG-12', 'Make the notification settings page load faster on slow phones')
  expect(long).toBe('eng-12-make-the-notification-settings-page-load-faster-on')
  expect(long.length - 'eng-12-'.length).toBeLessThanOrEqual(50)
  // one word that is longer than the limit is cut; a title with no letters gives the identifier
  expect(taskBranchName('ENG-12', 'x'.repeat(80))).toBe(`eng-12-${'x'.repeat(50)}`)
  expect(taskBranchName('ENG-12', '日本語 !!')).toBe('eng-12')
  expect(taskBranchName('ENG-12', '')).toBe('eng-12')
})
