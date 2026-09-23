import { expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import { useWorkspaceEvents } from './useWorkspaceEvents'

test('failed WebSocket handshakes do not repeatedly revalidate HTTP queries', () => {
  const originalWebSocket = globalThis.WebSocket
  const sockets: FakeWebSocket[] = []
  class FakeWebSocket {
    onopen: (() => void) | null = null
    onclose: (() => void) | null = null
    onmessage: ((event: MessageEvent) => void) | null = null
    constructor(_url: URL) { sockets.push(this) }
    close() { this.onclose?.() }
  }
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  const client = new QueryClient()
  const invalidate = mock(async (_options: { queryKey: readonly unknown[] }) => {})
  client.invalidateQueries = invalidate as typeof client.invalidateQueries

  try {
    const view = renderHook(() => useWorkspaceEvents('workspace-1'), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    })

    act(() => { sockets[0]!.onclose?.() })
    expect(invalidate).not.toHaveBeenCalled()
    view.unmount()

    // An established connection can close when the session or membership is revoked.
    const established = renderHook(() => useWorkspaceEvents('workspace-1'), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    })
    act(() => { sockets[1]!.onopen?.(); sockets[1]!.onclose?.() })
    expect(invalidate.mock.calls.map(([options]) => options.queryKey)).toEqual([
      queryKeys.currentUser,
      queryKeys.workspaces,
    ])
    established.unmount()
  } finally {
    globalThis.WebSocket = originalWebSocket
    client.clear()
  }
})

test('workspace metadata events refresh the workspace list, but task events do not', async () => {
  const originalWebSocket = globalThis.WebSocket
  let socket: { onmessage: ((event: MessageEvent) => void) | null } | undefined
  class FakeWebSocket {
    onmessage: ((event: MessageEvent) => void) | null = null
    constructor() { socket = this }
    close() {}
  }
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  const client = new QueryClient()
  const invalidate = mock(async (_options: { queryKey: readonly unknown[] }) => {})
  client.invalidateQueries = invalidate as typeof client.invalidateQueries
  try {
    const view = renderHook(() => useWorkspaceEvents('workspace-1'), {
      wrapper: ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>,
    })
    act(() => socket?.onmessage?.(new MessageEvent('message', {
      data: '{"version":1,"kind":"workspace.changed","sequence":"1","workspaces_changed":false}',
    })))
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(1))
    expect(invalidate.mock.calls[0]?.[0].queryKey).toEqual(queryKeys.workspace('workspace-1'))
    act(() => socket?.onmessage?.(new MessageEvent('message', {
      data: '{"version":1,"kind":"workspace.changed","sequence":"2","workspaces_changed":true}',
    })))
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(3))
    expect(invalidate.mock.calls[2]?.[0].queryKey).toEqual(queryKeys.workspaces)
    act(() => socket?.onmessage?.(new MessageEvent('message', {
      data: '{"version":1,"kind":"resync_required","sequence":"3"}',
    })))
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(6))
    expect(invalidate.mock.calls[4]?.[0].queryKey).toEqual(queryKeys.workspaces)
    expect(invalidate.mock.calls[5]?.[0].queryKey).toEqual(queryKeys.currentUser)
    act(() => socket?.onmessage?.(new MessageEvent('message', {
      data: '{"version":1,"kind":"workspace.changed","sequence":"4","profile_changed":true}',
    })))
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(8))
    expect(invalidate.mock.calls[7]?.[0].queryKey).toEqual(queryKeys.currentUser)
    view.unmount()
  } finally {
    client.clear()
    globalThis.WebSocket = originalWebSocket
  }
})

test('a failed workspace refresh backs off instead of retrying every 250 ms', async () => {
  const originalWebSocket = globalThis.WebSocket
  let socket: { onmessage: ((event: MessageEvent) => void) | null; close: () => void } | undefined
  class FakeWebSocket {
    onmessage: ((event: MessageEvent) => void) | null = null
    close() {}
    constructor(_url: URL) { socket = this }
  }
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  const client = new QueryClient()
  const invalidate = mock(async () => { throw new Error('API unavailable') })
  client.invalidateQueries = invalidate as typeof client.invalidateQueries

  try {
    const view = renderHook(() => useWorkspaceEvents('workspace-1'), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    })
    act(() => {
      socket!.onmessage?.(new MessageEvent('message', {
        data: '{"version":1,"kind":"workspace.changed","sequence":"1"}',
      }))
    })
    await new Promise((resolve) => setTimeout(resolve, 600))
    expect(invalidate).toHaveBeenCalledTimes(1)
    await new Promise((resolve) => setTimeout(resolve, 700))
    expect(invalidate).toHaveBeenCalledTimes(2)
    view.unmount()
  } finally {
    globalThis.WebSocket = originalWebSocket
    client.clear()
  }
})
