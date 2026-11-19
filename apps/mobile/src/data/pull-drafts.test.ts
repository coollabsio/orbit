import { expect, test } from 'bun:test'

import { clearDraft, readDraft, writeDraft } from './pull-drafts'

test('a draft stays for the app session until it is cleared', () => {
  expect(readDraft('k')).toBe('')
  writeDraft('k', 'Half a thought')
  expect(readDraft('k')).toBe('Half a thought')
  writeDraft('k', '')
  expect(readDraft('k')).toBe('')
  writeDraft('k', 'Again')
  clearDraft('k')
  expect(readDraft('k')).toBe('')
})
