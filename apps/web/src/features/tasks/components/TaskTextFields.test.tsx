import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import type { Task } from '@/features/tasks/api/models'
import { TaskTextFields } from './TaskTextFields'

function task(version: number, title: string, description: string): Task {
  return {
    id: 'task-1', version, title, description, statusId: 'todo', position: 0,
    projectId: 'project-1', identifier: 'ORB-1', priority: 'none', assigneeIds: [],
    creatorId: 'user-1', labels: [], attachments: [], dueAt: null, createdAt: '',
    updatedAt: '', comments: [], activity: [],
  }
}

test('authoritative conflict refresh replaces a visible title draft', () => {
  const onUpdate = mock(() => {})
  const view = render(<TaskTextFields task={task(1, 'Original', '')} onUpdate={onUpdate} />)
  fireEvent.click(view.getByText('Original'))
  fireEvent.change(view.getByLabelText('Task title'), { target: { value: 'Rejected title' } })

  view.rerender(<TaskTextFields task={task(2, 'Server title', '')} onUpdate={onUpdate} />)

  expect(view.getByText('Server title')).toBeTruthy()
})

test('web addresses in task titles become safe links', () => {
  const view = render(<TaskTextFields task={task(1, 'Review https://example.com/change', '')} onUpdate={() => {}} />)

  const link = view.getByRole('link') as HTMLAnchorElement
  expect(link.textContent).toBe('https://example.com/change')
  expect(link.href).toBe('https://example.com/change')
  expect(link.target).toBe('_blank')
  expect(link.rel).toContain('noreferrer')
})

test('the title field keeps the display text size on a narrow screen', async () => {
  const css = await Bun.file(new URL('../../../index.css', import.meta.url)).text()
  expect(css).toContain('input:not([data-keep-font-size])')

  const view = render(<TaskTextFields task={task(1, 'Editable title', '')} onUpdate={() => {}} />)
  fireEvent.click(view.getByText('Editable title'))
  expect(view.getByLabelText('Task title').hasAttribute('data-keep-font-size')).toBe(true)
})

test('a GitHub issue title is read-only, but links remain usable', () => {
  const onUpdate = mock(() => {})
  const view = render(<TaskTextFields task={task(1, 'Issue https://example.com/issue', '')} onUpdate={onUpdate} readOnly />)

  fireEvent.click(view.getByLabelText('Task title'))

  expect(view.queryByRole('textbox')).toBeNull()
  expect(view.getByRole('link')).toBeTruthy()
  expect(onUpdate).not.toHaveBeenCalled()
})

test('new untitled task opens with an empty focused title field', () => {
  const view = render(<TaskTextFields task={task(1, 'Untitled', '')} onUpdate={() => {}} />)
  const title = view.getByLabelText('Task title') as HTMLInputElement

  expect(title.value).toBe('')
  expect(title.placeholder).toBe('Task title')
  expect(document.activeElement).toBe(title)
})

test('Tab in the title hands focus to the description', () => {
  const onTitleTab = mock(() => {})
  const view = render(<TaskTextFields task={task(1, 'Untitled', '')} onUpdate={() => {}} onTitleTab={onTitleTab} />)

  fireEvent.keyDown(view.getByLabelText('Task title'), { key: 'Tab' })

  expect(onTitleTab).toHaveBeenCalledTimes(1)
})
