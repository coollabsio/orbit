import { afterEach, expect, mock, test } from 'bun:test'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { TaskMentionSearch } from './TaskMentionSearch'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

function respondWith(items: Array<{ id: string; identifier: string; title: string }>): string[] {
  const urls: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    urls.push(typeof input === 'string' ? input : (input as Request).url)
    return Response.json({ items, next_cursor: null }, { status: 200 })
  }) as unknown as typeof fetch
  return urls
}

const results = [
  { id: 'task-9', identifier: 'ORB-9', title: 'Gateway rewrite' },
  { id: 'task-12', identifier: 'ORB-12', title: 'Gateway timeouts' },
]

test('typing hits the relevance-sorted search and lists the issues it returns', async () => {
  const urls = respondWith(results)
  const view = render(<TaskMentionSearch workspaceId="workspace-1" onPick={() => {}} />)

  fireEvent.change(view.getByRole('textbox'), { target: { value: 'gate' } })

  await waitFor(() => expect(view.getByText('ORB-9')).toBeTruthy())
  expect(view.getByText('ORB-12')).toBeTruthy()
  expect(urls[0]).toContain('sort=relevance')
  expect(urls[0]).toContain('search=gate')
})

test('an empty query lists nothing and never reaches the network', async () => {
  const urls = respondWith(results)
  const view = render(<TaskMentionSearch workspaceId="workspace-1" onPick={() => {}} />)

  fireEvent.change(view.getByRole('textbox'), { target: { value: 'gate' } })
  fireEvent.change(view.getByRole('textbox'), { target: { value: '' } })

  await waitFor(() => expect(view.queryByRole('listbox')).toBeNull())
  expect(urls).toHaveLength(0)
})

test('the excluded task is never offered', async () => {
  respondWith(results)
  const view = render(<TaskMentionSearch workspaceId="workspace-1" excludeTaskId="task-9" onPick={() => {}} />)

  fireEvent.change(view.getByRole('textbox'), { target: { value: 'gate' } })

  await waitFor(() => expect(view.getByText('ORB-12')).toBeTruthy())
  expect(view.queryByText('ORB-9')).toBeNull()
})

test('ArrowDown moves the active option and Enter reports its task id', async () => {
  respondWith(results)
  const onPick = mock((_taskId: string) => {})
  const view = render(<TaskMentionSearch workspaceId="workspace-1" onPick={onPick} />)
  const input = view.getByRole('textbox')

  fireEvent.change(input, { target: { value: 'gate' } })
  await waitFor(() => expect(view.getAllByRole('option')).toHaveLength(2))

  expect(view.getAllByRole('option')[0].getAttribute('aria-selected')).toBe('true')
  fireEvent.keyDown(input, { key: 'ArrowDown' })
  expect(view.getAllByRole('option')[1].getAttribute('aria-selected')).toBe('true')

  fireEvent.keyDown(input, { key: 'Enter' })
  expect(onPick).toHaveBeenCalledWith('task-12')
})

test('Escape clears the query and the results instead of picking', async () => {
  respondWith(results)
  const onPick = mock((_taskId: string) => {})
  const view = render(<TaskMentionSearch workspaceId="workspace-1" onPick={onPick} />)
  const input = view.getByRole('textbox') as HTMLInputElement

  fireEvent.change(input, { target: { value: 'gate' } })
  await waitFor(() => expect(view.getAllByRole('option')).toHaveLength(2))

  fireEvent.keyDown(input, { key: 'Escape' })

  expect(input.value).toBe('')
  expect(view.queryByRole('listbox')).toBeNull()
  expect(onPick).not.toHaveBeenCalled()
})

test('the picker labels its own listbox rather than borrowing the mention menu label', async () => {
  respondWith(results)
  const view = render(<TaskMentionSearch workspaceId="workspace-1" onPick={() => {}} />)

  fireEvent.change(view.getByRole('textbox'), { target: { value: 'gate' } })

  await waitFor(() => expect(view.getByRole('listbox').getAttribute('aria-label')).toBe('Search issues'))
})
