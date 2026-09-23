import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { useGithubProjectSettings, useGithubWorkspaceSettings } from '@/features/tasks/api/github'
import { useWorkspaceEvents } from './useWorkspaceEvents'

test('workspace WebSocket changes refresh GitHub settings for every connected user', async () => {
  const originalFetch = globalThis.fetch
  const originalWebSocket = globalThis.WebSocket
  const sockets: FakeWebSocket[] = []
  let requests = 0
  class FakeWebSocket {
    onopen: (() => void) | null = null
    onclose: (() => void) | null = null
    onmessage: ((event: MessageEvent) => void) | null = null
    constructor() { sockets.push(this) }
    close() {}
  }
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  globalThis.fetch = (async () => {
    requests += 1
    return Response.json({ app_slug: 'orbit', repositories: [], can_manage: true, key_configured: true })
  }) as unknown as typeof fetch
  const clients = [0, 1].map(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  try {
    const views = clients.map((client) => renderHook(() => {
      useGithubWorkspaceSettings('workspace-1')
      useGithubProjectSettings('workspace-1', 'project-1')
      useWorkspaceEvents('workspace-1')
    }, { wrapper: ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> }))
    await waitFor(() => expect(requests).toBe(4))
    act(() => sockets.forEach((socket) => socket.onmessage?.(new MessageEvent('message', { data: '{"version":1,"kind":"workspace.changed","sequence":"1"}' }))))
    await waitFor(() => expect(requests).toBe(8))
    views.forEach((view) => view.unmount())
  } finally {
    clients.forEach((client) => client.clear())
    globalThis.fetch = originalFetch
    globalThis.WebSocket = originalWebSocket
  }
})
