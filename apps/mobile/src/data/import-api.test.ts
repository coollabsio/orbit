import { expect, test } from 'bun:test'
import type { LinearImport } from '@orbit/api/generated/types.gen'
import { linearImportProgress, linearRateLimitNotice } from './import-api'

const importing = (progress: Partial<LinearImport['progress']>, status: LinearImport['status'] = 'importing'): LinearImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status, linear_user_name: 'Ada', error: null, waiting_until: null,
  progress: { total: 8_000, done: 0, failed: 0, expected: 8_005, read: 0, created: 0, stage: null, ...progress },
  tree: null, report: null, project_ids: [], created_at: '2026-11-09T21:00:00Z', updated_at: '2026-11-09T21:00:00Z',
})

test('Linear progress names each stage with its own count', () => {
  expect(linearImportProgress(importing({}, 'queued'))).toEqual({ label: 'Waiting for the import to start…', step: null, done: 0, total: 0, detail: '' })
  expect(linearImportProgress(importing({ stage: 'reading', total: 605, read: 1_230 }))).toEqual({ label: 'Reading issues from Linear…', step: 1, done: 1_230, total: 8_005, detail: '1,230 of about 8,005 issues read' })
  expect(linearImportProgress(importing({ stage: 'reading', total: 605, read: 1_230, done: 300 })).detail).toBe('1,230 of about 8,005 issues read, 300 imported')
  expect(linearImportProgress(importing({ stage: 'creating', created: 2_500 }))).toEqual({ label: 'Creating tasks…', step: 2, done: 2_500, total: 8_000, detail: '2,500 of 8,000 tasks created' })
  expect(linearImportProgress(importing({ stage: 'details', done: 40, failed: 2 }))).toEqual({ label: 'Adding files and comments…', step: 3, done: 42, total: 8_000, detail: '40 of 8,000 issues imported' })
  expect(linearImportProgress(importing({ stage: 'linking' })).step).toBe(4)
})

test('the rate-limit notice shows only while the wait lasts', () => {
  const now = Date.parse('2026-11-09T21:00:00Z')
  expect(linearRateLimitNotice(null, now)).toBeNull()
  expect(linearRateLimitNotice('2026-11-09T20:59:00Z', now)).toBeNull()
  expect(linearRateLimitNotice('2026-11-09T21:11:30Z', now)).toMatch(/\(in about 12 min\)\.$/)
})
