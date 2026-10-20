import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { CollabConnectContext } from '@/features/docs/collab/connection'
import type { Task } from '@/features/tasks/api/models'
import { fakeCollab } from '@/test/fakeCollab'
import { TaskDescriptionEditor } from './TaskDescriptionEditor'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const task: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Task', description: 'Start', collabEpoch: 'epoch-1', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
}

test('a task that is back after a 404 close (restored, access returned) reconnects its description', async () => {
  // every read answers: the task exists again
  globalThis.fetch = (async () => Response.json({ id: 'task-1' })) as unknown as typeof fetch
  const collab = fakeCollab({ content: () => [{ type: 'paragraph', content: [{ type: 'text', text: 'Start', styles: {} }] }] })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <CollabConnectContext value={collab.connect}>{children}</CollabConnectContext>
      </MemoryRouter>
    </QueryClientProvider>
  )
  render(<TaskDescriptionEditor workspaceId="workspace-1" task={task} readOnly={false} />, { wrapper })
  await waitFor(() => expect(collab.connections).toHaveLength(1))

  await act(async () => collab.last().provider.drop(4404))

  await waitFor(() => expect(collab.connections).toHaveLength(2))
})
