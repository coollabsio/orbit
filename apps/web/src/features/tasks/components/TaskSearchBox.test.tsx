import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { TaskSearchBox } from './TaskSearchBox'

test('task search is accessible, keeps its content width, and reports changes', async () => {
  function View() {
    const [value, setValue] = useState('')
    return <TaskSearchBox value={value} onChange={setValue} />
  }
  const view = render(<View />)
  const box = view.getByRole('searchbox', { name: 'Search tasks' })
  await userEvent.type(box, 'release')
  expect((box as HTMLInputElement).value).toBe('release')
  const group = box.closest('[data-slot="input-group"]')
  expect(group?.classList.contains('w-auto')).toBe(true)
  expect(group?.classList.contains('w-full')).toBe(false)
})
