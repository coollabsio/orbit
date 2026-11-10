import { afterEach, beforeEach, expect, jest, test } from 'bun:test'
import { act, renderHook } from '@testing-library/react'
import { useHoverPreview } from './useHoverPreview'

beforeEach(() => jest.useFakeTimers())
afterEach(() => jest.useRealTimers())

const rect = new DOMRect(10, 20, 30, 16)

test('a preview opens after the pointer rests, closes at once, and swaps without the wait while one is open', () => {
  const { result } = renderHook(() => useHoverPreview<string>())
  act(() => result.current.show('a', 'task:ENG-1', rect))
  expect(result.current.shown).toBeNull()
  act(() => jest.advanceTimersByTime(400))
  expect(result.current.shown?.value).toBe('task:ENG-1')

  act(() => result.current.show('b', 'user:u1', rect))
  act(() => jest.advanceTimersByTime(0))
  expect(result.current.shown?.value).toBe('user:u1')

  act(() => result.current.hide())
  expect(result.current.shown).toBeNull()
})

test('leaving before the wait is over opens nothing', () => {
  const { result } = renderHook(() => useHoverPreview<string>())
  act(() => result.current.show('a', 'task:ENG-1', rect))
  act(() => result.current.hide())
  act(() => jest.advanceTimersByTime(400))
  expect(result.current.shown).toBeNull()
})
