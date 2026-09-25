import { afterEach, expect, test } from 'bun:test'
import { act, renderHook } from '@testing-library/react'
import { useCollapsedGroups } from './useCollapsedGroups'

const KEY = 'orbit:task_list_collapsed:alpha:all'
const originalSetItem = window.localStorage.setItem
/** `defineProperty`, not assignment: assigning on a Storage only stores an item named `setItem`. */
const replaceSetItem = (setItem: Storage['setItem']) =>
  Object.defineProperty(window.localStorage, 'setItem', { value: setItem, configurable: true, writable: true })
afterEach(() => {
  replaceSetItem(originalSetItem)
  localStorage.clear()
})

test('collapsed groups are remembered per storage key', () => {
  const view = renderHook(() => useCollapsedGroups(KEY))
  act(() => view.result.current[1]('todo'))
  act(() => view.result.current[1]('done'))
  act(() => view.result.current[1]('todo'))
  expect(view.result.current[0]).toEqual(['done'])
  expect(JSON.parse(localStorage.getItem(KEY) ?? 'null')).toEqual(['done'])
  view.unmount()
  expect(renderHook(() => useCollapsedGroups(KEY)).result.current[0]).toEqual(['done'])
})

test('a storage write that throws (quota, private mode) still collapses the group', () => {
  let writes = 0
  replaceSetItem(() => {
    writes += 1
    throw new DOMException('Quota exceeded', 'QuotaExceededError')
  })
  const view = renderHook(() => useCollapsedGroups(KEY))
  act(() => view.result.current[1]('todo'))
  expect(writes).toBeGreaterThan(0)
  expect(view.result.current[0]).toEqual(['todo'])
})
