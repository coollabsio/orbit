import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { TaskFilters } from './TaskFilters'

test('task search is accessible and reports changes', async () => {
  function View() {
    const [value, setValue] = useState('')
    return <TaskFilters sort="manual" layout="list" search={value} onSearchChange={setValue} onSortChange={() => {}} onLayoutChange={() => {}} />
  }
  const view = render(<View />)
  await userEvent.type(view.getByRole('searchbox', { name: 'Search tasks' }), 'release')
  expect((view.getByRole('searchbox', { name: 'Search tasks' }) as HTMLInputElement).value).toBe('release')
})

test('display offers a timeline layout and hides sort while it is active', async () => {
  function View() {
    const [layout, setLayout] = useState<'list' | 'board' | 'timeline'>('list')
    return <TaskFilters sort="manual" layout={layout} search="" onSearchChange={() => {}} onSortChange={() => {}} onLayoutChange={setLayout} />
  }
  const view = render(<View />)
  expect(view.getByRole('button', { name: /^Sort tasks/ })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Display options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Timeline' }))
  expect(view.queryAllByRole('button', { name: /^Sort tasks/ }).length).toBe(0)
})
