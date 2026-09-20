import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { TaskFilters } from './TaskFilters'

test('task search is accessible and reports changes', async () => {
  function View() {
    const [value, setValue] = useState('')
    return <TaskFilters users={[]} labels={[]} groups={[]} statusKey={null} assigneeId={null} labelId={null} priority={null} sort="manual" layout="list"
      search={value} onSearchChange={setValue} onStatusChange={() => {}} onAssigneeChange={() => {}}
      onLabelChange={() => {}} onPriorityChange={() => {}}
      onSortChange={() => {}} onLayoutChange={() => {}} />
  }
  const view = render(<View />)
  await userEvent.type(view.getByRole('searchbox', { name: 'Search tasks' }), 'release')
  expect((view.getByRole('searchbox', { name: 'Search tasks' }) as HTMLInputElement).value).toBe('release')
})
