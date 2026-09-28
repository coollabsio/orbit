import { afterEach, expect, test } from 'bun:test'
import { act, renderHook } from '@testing-library/react'
import { useCollapsedTasks } from './useCollapsedTasks'

afterEach(() => window.localStorage.clear())

test('trees start expanded; collapsing is remembered per workspace and expand undoes it', () => {
  const first = renderHook(() => useCollapsedTasks('workspace-1'))
  expect(first.result.current.collapsed.size).toBe(0)
  act(() => first.result.current.toggle('task-1'))
  expect(first.result.current.collapsed.has('task-1')).toBe(true)
  expect(window.localStorage.getItem('orbit:task_tree_collapsed:workspace-1')).toBe('["task-1"]')
  const again = renderHook(() => useCollapsedTasks('workspace-1'))
  expect(again.result.current.collapsed.has('task-1')).toBe(true)
  expect(renderHook(() => useCollapsedTasks('workspace-2')).result.current.collapsed.size).toBe(0)
  act(() => again.result.current.expand('task-1'))
  expect(window.localStorage.getItem('orbit:task_tree_collapsed:workspace-1')).toBe('[]')
})

test('the list and the detail share one state, so a toggle in one never overwrites the other', () => {
  const list = renderHook(() => useCollapsedTasks('workspace-1'))
  const detail = renderHook(() => useCollapsedTasks('workspace-1'))
  act(() => detail.result.current.toggle('task-x'))
  act(() => list.result.current.toggle('task-y'))
  expect([...list.result.current.collapsed]).toEqual(['task-x', 'task-y'])
  expect([...detail.result.current.collapsed]).toEqual(['task-x', 'task-y'])
  expect(window.localStorage.getItem('orbit:task_tree_collapsed:workspace-1')).toBe('["task-x","task-y"]')
})
