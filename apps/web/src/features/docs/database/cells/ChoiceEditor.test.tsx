import { expect, test } from 'bun:test'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { CellValue, Property, Row, SelectOption } from '@orbit/database/types'
import { render } from '@/test/render'
import { CellEditor } from './CellEditor'

const property = {
  id: 'p-stage', name: 'Stage', type: 'select', position: 1,
  config: { options: [{ id: 'o1', name: 'Alpha', color: 'blue' }] },
} as unknown as Property
const row = {
  id: 'r1', title: 'Row', icon: null, position: 0, version: 0, has_content: false,
  created_at: '2026-11-09T10:00:00Z', created_by: 'u1', updated_at: '2026-11-09T10:00:00Z', updated_by: 'u1', values: {},
} as Row

function setup() {
  const saved: SelectOption[][] = []
  const committed: (CellValue | null)[] = []
  render(
    <CellEditor
      property={property}
      row={row}
      anchor={document.body}
      onCommit={(value) => committed.push(value)}
      onTitle={() => {}}
      onOptions={async (options) => { saved.push(options) }}
      onClose={() => {}}
    />,
  )
  return { saved, committed }
}

const optionTexts = () => Array.from(document.querySelectorAll('[cmdk-item]'), (item) => item.textContent)

test('the Create row shows for every typed character, also when no option matches', async () => {
  const user = userEvent.setup()
  setup()
  const input = await screen.findByLabelText('Search Stage')
  let typed = ''
  for (const char of 'Alx') {
    typed += char
    await user.type(input, char)
    expect(optionTexts()).toEqual(typed === 'Alx' ? ['CreateAlx'] : ['Alpha', `Create${typed}`])
  }
  expect(screen.queryAllByText('No options')).toHaveLength(0)
})

test('Enter creates the typed option when no option matches, and picks it', async () => {
  const user = userEvent.setup()
  const { saved, committed } = setup()
  await user.type(await screen.findByLabelText('Search Stage'), 'New stage{Enter}')
  expect(saved).toHaveLength(1)
  expect(saved[0].map((option) => option.name)).toEqual(['Alpha', 'New stage'])
  expect(committed).toEqual([{ kind: 'choice', id: saved[0][1].id }])
})
