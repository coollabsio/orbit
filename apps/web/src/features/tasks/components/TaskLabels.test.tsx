import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { TaskLabels } from './TaskLabels'

const labels: LabelRecord[] = [
  { id: 'label-used', workspace_id: 'workspace-1', name: 'Bug', color: '#ff0000', version: 0 },
  { id: 'label-unused', workspace_id: 'workspace-1', name: 'Needs review', color: '#123456', version: 0 },
]

test('task labels render names and colors and offer unused workspace labels', () => {
  const view = render(<TaskLabels labelIds={['label-used']} labels={labels} onChange={mock(() => {})} />)

  expect(view.getByText('Bug')).toBeTruthy()
  expect(view.queryByText('label-used')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Add label' }))
  expect(view.getByRole('button', { name: /Needs review/ })).toBeTruthy()
})
