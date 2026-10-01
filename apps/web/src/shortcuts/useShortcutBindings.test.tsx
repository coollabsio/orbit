import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { useShortcutBindings } from './useShortcutBindings'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const wrapper = ({ children }: { children: ReactNode }) => (
  <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>{children}</QueryClientProvider>
)

test('overrides that cannot be loaded leave the defaults in effect', async () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return Response.json({ code: 'internal_error' }, { status: 500 })
  }) as unknown as typeof fetch
  const { result } = renderHook(() => useShortcutBindings(), { wrapper })
  await waitFor(() => expect(calls).toBe(1))
  expect(result.current.overrides).toEqual({})
})

test('a refused save restores the overrides that were there', async () => {
  let puts = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    if ((input as Request).method !== 'PUT') return Response.json({ bindings: { 'task.create': 'N' } })
    puts += 1
    return Response.json({ code: 'internal_error' }, { status: 500 })
  }) as unknown as typeof fetch
  const { result } = renderHook(() => useShortcutBindings(), { wrapper })
  await waitFor(() => expect(result.current.overrides).toEqual({ 'task.create': 'N' }))
  act(() => result.current.save({ 'task.create': 'M' }))
  await waitFor(() => expect(puts).toBe(1))
  await waitFor(() => expect(result.current.isSaving).toBe(false))
  expect(result.current.overrides).toEqual({ 'task.create': 'N' })
})
