import { expect, test } from 'bun:test'

import { projectDraft } from './orbit'

test('a new project gets a key of up to five letters and digits from its name, as on the web', () => {
  expect(projectDraft('Orbit Web')).toEqual({ name: 'Orbit Web', key: 'ORBIT', color: '#8b5cf6' })
  expect(projectDraft('Q3 roadmap')).toMatchObject({ key: 'Q3ROA' })
  expect(projectDraft('🚀 —')).toMatchObject({ key: 'PROJ' })
})

test('a derived key that another project has gets the smallest free number, as on the web (#85)', () => {
  expect(projectDraft('General - Y', ['GENER']).key).toBe('GENER2')
  expect(projectDraft('General - Z', ['GENER', 'gener2', 'OTHER']).key).toBe('GENER3')
  expect(projectDraft('Y - General', ['GENER']).key).toBe('YGENE')
})
