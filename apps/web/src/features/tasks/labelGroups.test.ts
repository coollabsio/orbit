import { expect, test } from 'bun:test'
import { addLabel, labelDisplayName, labelSections, toggleLabel } from './labelGroups'

const label = (id: string, group: string | null = null) => ({ id, name: id, group_id: group, group_name: group ? `Group ${group}` : null })
const LABELS = [label('bug', 't'), label('feature', 't'), label('ui'), label('high', 'p'), label('api')]

test('adding a label of a group replaces the other label of that group only', () => {
  expect(addLabel(['bug', 'ui', 'high'], 'feature', LABELS)).toEqual(['ui', 'high', 'feature'])
  expect(addLabel(['bug'], 'ui', LABELS)).toEqual(['bug', 'ui'])
  expect(toggleLabel(['bug', 'ui'], 'bug', LABELS)).toEqual(['ui'])
  expect(toggleLabel(['feature'], 'bug', LABELS)).toEqual(['bug'])
})

test('sections list the groups by name and the labels with no group last', () => {
  expect(labelSections(LABELS).map((section) => [section.name, section.labels.map((item) => item.id)])).toEqual([
    ['Group p', ['high']],
    ['Group t', ['bug', 'feature']],
    [null, ['api', 'ui']],
  ])
  expect(labelDisplayName(LABELS[0]!)).toBe('Group t / bug')
  expect(labelDisplayName(LABELS[2]!)).toBe('ui')
})
