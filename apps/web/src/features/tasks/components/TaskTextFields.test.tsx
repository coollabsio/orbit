import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
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

test('authoritative conflict refresh replaces visible controlled title and description drafts', () => {
  const onUpdate = mock(() => {})
  const view = render(<TaskTextFields task={task(1, 'Original', 'Old description')} onUpdate={onUpdate} />)
  fireEvent.click(view.getByText('Original'))
  fireEvent.change(view.getByLabelText('Task title'), { target: { value: 'Rejected title' } })
  fireEvent.click(view.getByText('Old description'))
  fireEvent.change(view.getByLabelText('Description'), { target: { value: 'Rejected description' } })

  view.rerender(<TaskTextFields task={task(2, 'Server title', 'Server description')} onUpdate={onUpdate} />)

  expect(view.getByText('Server title')).toBeTruthy()
  expect(view.getByText('Server description')).toBeTruthy()
})

test('web addresses in task titles and descriptions become safe links', () => {
  const view = render(
    <TaskTextFields
      task={task(1, 'Review https://example.com/change', 'Details at https://example.com/docs.')}
      onUpdate={() => {}}
    />,
  )

  const links = view.getAllByRole('link') as HTMLAnchorElement[]
  expect(links.map((link) => link.textContent)).toEqual([
    'https://example.com/change',
    'https://example.com/docs',
  ])
  expect(links.map((link) => link.href)).toEqual([
    'https://example.com/change',
    'https://example.com/docs',
  ])
  expect(links.every((link) => link.target === '_blank')).toBe(true)
  expect(links.every((link) => link.rel.includes('noreferrer'))).toBe(true)
})

test('task description renders Markdown after editing but shows raw Markdown while editing', async () => {
  const onUpdate = mock(() => {})
  const description = '## Plan\n- **First** step\n- [Details](https://example.com/plan)'
  const view = render(<TaskTextFields task={task(1, 'Task', description)} onUpdate={onUpdate} />)

  expect(view.getByRole('heading', { name: 'Plan', level: 2 })).toBeTruthy()
  expect(view.getByText('First').tagName).toBe('STRONG')
  expect(view.getByRole('list').children).toHaveLength(2)

  fireEvent.click(view.getByRole('link', { name: 'Details' }))
  expect(view.container.querySelector('textarea[aria-label="Description"]')).toBeNull()

  fireEvent.click(view.getByRole('heading', { name: 'Plan' }))
  const editor = view.getByRole('textbox', { name: 'Description' }) as HTMLTextAreaElement
  expect(editor.value).toBe(description)
  await userEvent.clear(editor)
  await userEvent.type(editor, '**Done**')
  expect(editor.value).toBe('**Done**')
  fireEvent.blur(editor)
  expect(view.container.querySelector('textarea[aria-label="Description"]')).toBeNull()

  expect(onUpdate).toHaveBeenCalledWith({ description: '**Done**' })
  expect(view.getByText('Done').tagName).toBe('STRONG')
  expect(view.queryByText('**Done**')).toBeNull()
})

test('read-only task descriptions render Markdown and keep links usable', () => {
  const view = render(<TaskTextFields task={task(1, 'Issue', '# Issue\n[Source](https://example.com/issue)')} onUpdate={() => {}} readOnly />)

  expect(view.getByRole('heading', { name: 'Issue', level: 1 })).toBeTruthy()
  expect((view.getByRole('link', { name: 'Source' }) as HTMLAnchorElement).href).toBe('https://example.com/issue')
  expect(view.queryByRole('textbox')).toBeNull()
})

test('task description preview renders checklists, nested lists, tables, strikethrough, and dividers', () => {
  const description = '- [x] Done\n  - Child\n- [ ] Next\n\n| Item | State |\n| --- | --- |\n| One | ~~Old~~ |\n\n---'
  const view = render(<TaskTextFields task={task(1, 'Preview', description)} onUpdate={() => {}} readOnly />)

  expect((view.getByRole('checkbox', { name: 'Done' }) as HTMLInputElement).checked).toBe(true)
  expect((view.getByRole('checkbox', { name: 'Next' }) as HTMLInputElement).checked).toBe(false)
  expect(view.getByText('Child').closest('ul')?.parentElement?.textContent).toContain('Done')
  expect(view.getByRole('table').querySelectorAll('tbody tr')).toHaveLength(1)
  expect(view.getByText('Old').tagName).toBe('DEL')
  expect(view.container.querySelector('hr')).toBeTruthy()
})

test('task descriptions show Markdown and GitHub HTML images within the preview width', () => {
  const description = '![Diagram](https://example.com/diagram.png)\n<img width="2205" height="119" alt="Screenshot" src="https://github.com/user-attachments/assets/example" />'
  const view = render(<TaskTextFields task={task(1, 'Images', description)} onUpdate={() => {}} readOnly />)

  const images = view.getAllByRole('img') as HTMLImageElement[]
  expect(images.map((item) => item.alt)).toEqual(['Diagram', 'Screenshot'])
  expect(images.map((item) => item.src)).toEqual([
    'https://example.com/diagram.png',
    'https://github.com/user-attachments/assets/example',
  ])
  expect(images.every((item) => item.classList.contains('max-w-full'))).toBe(true)
  expect(view.queryByText(/<img width/)).toBeNull()
})

test('unsafe HTML image sources stay text, not active images', () => {
  const description = '<img alt="Unsafe" src="javascript:alert(1)" onerror="alert(1)" />'
  const view = render(<TaskTextFields task={task(1, 'Unsafe image', description)} onUpdate={() => {}} readOnly />)

  expect(view.queryByRole('img')).toBeNull()
  expect(view.getByText(description)).toBeTruthy()
})

test('edit fields keep the display text size on a narrow screen', async () => {
  const css = await Bun.file(new URL('../../../index.css', import.meta.url)).text()
  expect(css).toContain('input:not([data-keep-font-size])')
  expect(css).toContain('textarea:not([data-keep-font-size])')

  const view = render(<TaskTextFields task={task(1, 'Editable title', 'Editable description')} onUpdate={() => {}} />)
  fireEvent.click(view.getByText('Editable title'))
  expect(view.getByLabelText('Task title').hasAttribute('data-keep-font-size')).toBe(true)
  fireEvent.click(view.getByText('Editable description'))
  expect(view.getByLabelText('Description').hasAttribute('data-keep-font-size')).toBe(true)
})

test('plain task text stays editable when clicked', () => {
  const view = render(<TaskTextFields task={task(1, 'Editable title', 'Editable description')} onUpdate={() => {}} />)

  fireEvent.click(view.getByText('Editable title'))
  expect(view.getByLabelText('Task title')).toBeTruthy()
  fireEvent.click(view.getByText('Editable description'))
  const description = view.getByLabelText('Description')
  expect(description.classList.contains('block')).toBe(true)
  expect(description.classList.contains('inline-block')).toBe(false)
})

test('GitHub issue title and description are read-only, but links remain usable', () => {
  const onUpdate = mock(() => {})
  const view = render(<TaskTextFields task={task(1, 'Issue title', 'See https://example.com/issue')} onUpdate={onUpdate} readOnly />)

  fireEvent.click(view.getByLabelText('Task title'))
  fireEvent.keyDown(view.getByLabelText('Description'), { key: 'Enter' })

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

test('Tab moves focus from a new task title to its description', () => {
  const view = render(<TaskTextFields task={task(1, 'Untitled', '')} onUpdate={() => {}} />)
  const title = view.getByLabelText('Task title')

  fireEvent.keyDown(title, { key: 'Tab' })

  expect(document.activeElement).toBe(view.getByLabelText('Description'))
})
