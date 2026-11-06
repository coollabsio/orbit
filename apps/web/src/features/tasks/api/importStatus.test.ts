import { expect, test } from 'bun:test'
import { IMPORT_STATUS_LABELS, isActiveImport, isFinalImport, type TaskImportStatus } from './importStatus'

test('active imports poll, final ones never change', () => {
  const statuses = Object.keys(IMPORT_STATUS_LABELS) as TaskImportStatus[]
  expect(statuses.filter(isActiveImport)).toEqual(['scanning', 'queued', 'importing'])
  expect(statuses.filter(isFinalImport)).toEqual(['completed', 'failed', 'cancelled', 'expired'])
  expect(isActiveImport(undefined)).toBe(false)
})
