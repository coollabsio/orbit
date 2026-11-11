import { describe, expect, test } from 'bun:test'
import { buttonLabel, parseButtonConfig, type DatabaseButtonAction } from './buttons'

describe('parseButtonConfig', () => {
  test('a stored config keeps its label, actions and confirmation', () => {
    const actions: DatabaseButtonAction[] = [
      { type: 'set_values', values: { 'p-status': 's-done', 'p-date': 'today' } },
      { type: 'add_row', database_id: 'db-log', values: { 'p-title': 'Closed' } },
      { type: 'open_url', url: 'https://example.com/done' },
    ]
    expect(parseButtonConfig({ label: 'Mark done', actions, confirm: 'Close this task?' })).toEqual({ label: 'Mark done', actions, confirm: 'Close this task?' })
  })

  test('malformed actions are dropped and the rest falls back to the defaults', () => {
    const config = {
      label: 7,
      confirm: '',
      actions: [null, 'set_values', { type: 'add_row' }, { type: 'open_url', url: 3 }, { type: 'notify' }, { type: 'open_url', url: 'https://a.test' }],
    }
    expect(parseButtonConfig(config)).toEqual({ label: '', actions: [{ type: 'open_url', url: 'https://a.test' }], confirm: null })
    expect(parseButtonConfig({ actions: { type: 'set_values' } })).toEqual({ label: '', actions: [], confirm: null })
  })
})

test('buttonLabel shows the label, or the property name when it is blank', () => {
  const config = parseButtonConfig({})
  expect([buttonLabel({ name: 'Close', config: { ...config, label: ' Done ' } }), buttonLabel({ name: 'Close', config: { ...config, label: '  ' } })]).toEqual(['Done', 'Close'])
})
