import { expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import { focusedDraftBlocksRefresh, REALTIME_SAFE_ATTRIBUTE, useWorkspaceEvents } from './useWorkspaceEvents'

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

test('any workspace event refreshes saved views and task queries but never view preferences', async () => {
  const originalWebSocket = globalThis.WebSocket
  let socket: { onmessage: ((event: MessageEvent) => void) | null } | undefined
  class FakeWebSocket {
    onmessage: ((event: MessageEvent) => void) | null = null
    constructor() { socket = this }
    close() {}
  }
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  const client = new QueryClient()
  const body = { filter: { op: 'and', children: [] }, order_by: 'manual', order_direction: 'asc', show_completed: 'all' }
  const keys = [
    queryKeys.views('workspace-1'),
    queryKeys.view('workspace-1', 'view-1'),
    queryKeys.viewPreference('workspace-1', 'all'),
    queryKeys.taskQuery('workspace-1', body),
  ]
  for (const key of keys) client.setQueryData(key, {})
  try {
    const view = renderHook(() => useWorkspaceEvents('workspace-1'), {
      wrapper: ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>,
    })
    // saved_view.* audit events reach the socket as a plain workspace.changed (no event name)
    act(() => socket?.onmessage?.(new MessageEvent('message', {
      data: '{"version":1,"kind":"workspace.changed","sequence":"1"}',
    })))
    // preference writes are never broadcast, and a refetch could land the old value over a debounced edit
    await waitFor(() => expect(keys.map((key) => client.getQueryState(key)?.isInvalidated)).toEqual([true, true, false, true]))
    act(() => socket?.onmessage?.(new MessageEvent('message', {
      data: '{"version":1,"kind":"resync_required","sequence":"2"}',
    })))
    await settleRefresh()
    expect(client.getQueryState(queryKeys.viewPreference('workspace-1', 'all'))?.isInvalidated).toBeFalse()
    view.unmount()
  } finally {
    globalThis.WebSocket = originalWebSocket
    client.clear()
  }
})

/** Waits past the hook's 250 ms refresh flush. */
const settleRefresh = () => act(() => new Promise<void>((resolve) => setTimeout(resolve, 400)))

test('a batch of only saved-view changes refreshes just the views; anything else refreshes the workspace', async () => {
  const originalWebSocket = globalThis.WebSocket
  let socket: { onmessage: ((event: MessageEvent) => void) | null } | undefined
  class FakeWebSocket {
    onmessage: ((event: MessageEvent) => void) | null = null
    constructor() { socket = this }
    close() {}
  }
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  const client = new QueryClient()
  const body = { filter: { op: 'and', children: [] }, order_by: 'manual', order_direction: 'asc', show_completed: 'all' }
  const keys = [
    queryKeys.views('workspace-1'),
    queryKeys.view('workspace-1', 'view-1'),
    queryKeys.taskQuery('workspace-1', body),
    queryKeys.members('workspace-1'),
    queryKeys.viewPreference('workspace-1', 'all'),
  ]
  const reset = () => { for (const key of keys) client.setQueryData(key, {}) }
  const invalidated = () => keys.map((key) => client.getQueryState(key)?.isInvalidated)
  const send = (data: string) => act(() => socket?.onmessage?.(new MessageEvent('message', { data })))
  const input = document.createElement('input')
  document.body.append(input)
  reset()
  try {
    const view = renderHook(() => useWorkspaceEvents('workspace-1'), {
      wrapper: ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>,
    })
    send('{"version":1,"kind":"workspace.changed","sequence":"1","views_only":true}')
    await waitFor(() => expect(invalidated()).toEqual([true, true, false, false, false]))

    // a missing flag (an older server) counts as false
    reset()
    send('{"version":1,"kind":"workspace.changed","sequence":"2"}')
    await waitFor(() => expect(invalidated()).toEqual([true, true, true, true, false]))

    // a views-only event coalesced with a task change still refreshes the workspace
    reset()
    input.focus()
    send('{"version":1,"kind":"workspace.changed","sequence":"3","views_only":true}')
    send('{"version":1,"kind":"workspace.changed","sequence":"4","views_only":false}')
    send('{"version":1,"kind":"workspace.changed","sequence":"5","views_only":true}')
    await settleRefresh()
    expect(invalidated()).toEqual([false, false, false, false, false])
    input.blur()
    await waitFor(() => expect(invalidated()).toEqual([true, true, true, true, false]))
    view.unmount()
  } finally {
    input.remove()
    globalThis.WebSocket = originalWebSocket
    client.clear()
  }
})

test('a focused draft pauses refreshes unless it sits inside a realtime-safe surface', () => {
  const form = document.createElement('form')
  const input = document.createElement('input')
  form.append(input)
  const safe = document.createElement('section')
  safe.setAttribute(REALTIME_SAFE_ATTRIBUTE, '')
  const title = document.createElement('input')
  const editor = document.createElement('div')
  editor.setAttribute('contenteditable', 'true')
  safe.append(title, editor)
  const button = document.createElement('button')
  document.body.append(form, safe, button)
  try {
    expect(focusedDraftBlocksRefresh(input)).toBe(true)
    expect(focusedDraftBlocksRefresh(title)).toBe(false)
    expect(focusedDraftBlocksRefresh(editor)).toBe(false)
    expect(focusedDraftBlocksRefresh(button)).toBe(false)
    expect(focusedDraftBlocksRefresh(null)).toBe(false)
  } finally {
    form.remove()
    safe.remove()
    button.remove()
  }
})
