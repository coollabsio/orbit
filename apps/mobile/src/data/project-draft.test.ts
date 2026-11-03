import { expect, test } from 'bun:test'

import { projectDraft } from './orbit'

test('a new project gets a key of up to five letters and digits from its name, as on the web', () => {
  expect(projectDraft('Orbit Web')).toEqual({ name: 'Orbit Web', key: 'ORBIT', color: '#8b5cf6' })
  expect(projectDraft('Q3 roadmap')).toMatchObject({ key: 'Q3ROA' })
  expect(projectDraft('🚀 —')).toMatchObject({ key: 'PROJ' })
})
