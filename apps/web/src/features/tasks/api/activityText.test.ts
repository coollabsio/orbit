import { expect, test } from 'bun:test'
import { activityText } from './models'

test('existing single-word actions keep their current wording', () => {
  expect(activityText('task.created')).toBe('Created task')
  expect(activityText('task.updated')).toBe('Updated task')
  expect(activityText('comment.deleted')).toBe('Deleted comment')
})

test('multi-word actions read as sentences, never raw audit names', () => {
  expect(activityText('task.duplicate_marked')).toBe('Marked as duplicate')
  expect(activityText('task.duplicate_unmarked')).toBe('Unmarked as duplicate')
  expect(activityText('task.bulk_updated')).toBe('Updated task')
  // An action added later without a label still never shows an underscore.
  expect(activityText('task.something_new')).toBe('Something new task')
})
