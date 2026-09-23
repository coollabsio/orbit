import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { TaskFilters } from './TaskFilters'

test('task search is accessible and reports changes', async () => {
  function View() {
    const [value, setValue] = useState('')
    return <TaskFilters users={[]} labels={[]} groups={[]} statusKey={null} assigneeId={null} unassigned={false} labelId={null} priority={null} sort="manual" layout="list"
      search={value} onSearchChange={setValue} onStatusChange={() => {}} onAssigneeChange={() => {}}
      onUnassignedChange={() => {}}
      onLabelChange={() => {}} onPriorityChange={() => {}}
      onSortChange={() => {}} onLayoutChange={() => {}} />
  }
  const view = render(<View />)
  await userEvent.type(view.getByRole('searchbox', { name: 'Search tasks' }), 'release')
  expect((view.getByRole('searchbox', { name: 'Search tasks' }) as HTMLInputElement).value).toBe('release')
})

test('unassigned is available as an assignee filter', async () => {
  function View() {
    const [unassigned, setUnassigned] = useState(false)
    return <TaskFilters users={[]} labels={[]} groups={[]} statusKey={null} assigneeId={null} unassigned={unassigned} labelId={null} priority={null} sort="manual" layout="list"
      search="" onSearchChange={() => {}} onStatusChange={() => {}} onAssigneeChange={() => {}}
      onUnassignedChange={setUnassigned} onLabelChange={() => {}} onPriorityChange={() => {}}
      onSortChange={() => {}} onLayoutChange={() => {}} />
  }
  const view = render(<View />)
  fireEvent.click(view.getByRole('button', { name: 'Filter tasks' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Unassigned' }))
  expect(view.getByRole('button', { name: 'Filter tasks' }).textContent).toContain('1')
})

test('display offers a timeline layout and hides sort while it is active', async () => {
  function View() {
    const [layout, setLayout] = useState<'list' | 'board' | 'timeline'>('list')
    return <TaskFilters users={[]} labels={[]} groups={[]} statusKey={null} assigneeId={null} unassigned={false} labelId={null} priority={null} sort="manual" layout={layout}
      search="" onSearchChange={() => {}} onStatusChange={() => {}} onAssigneeChange={() => {}}
      onUnassignedChange={() => {}} onLabelChange={() => {}} onPriorityChange={() => {}}
      onSortChange={() => {}} onLayoutChange={setLayout} />
  }
  const view = render(<View />)
  expect(view.getByRole('button', { name: /^Sort tasks/ })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Display options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Timeline' }))
  expect(view.queryByRole('button', { name: /^Sort tasks/ })).toBeNull()
})
