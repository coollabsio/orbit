import { afterEach, expect, test } from 'bun:test'
import { act, renderHook } from '@testing-library/react'
import { readTaskPreferences, useTaskPreferences } from './taskPreferences'

afterEach(() => localStorage.clear())

test('task filters and sort survive a remount in the same workspace', () => {
  const first = renderHook(() => useTaskPreferences('workspace-a'))
  act(() => first.result.current[1]({
    sort: 'priority',
    statusFilter: 'todo',
    assigneeFilter: null,
    unassignedFilter: true,
    labelFilter: 'label-a',
    priorityFilter: 'high',
    searchFilter: 'release',
  }))
  first.unmount()

  const second = renderHook(() => useTaskPreferences('workspace-a'))
  expect(second.result.current[0]).toEqual({
    sort: 'priority',
    statusFilter: 'todo',
    assigneeFilter: null,
    unassignedFilter: true,
    labelFilter: 'label-a',
    priorityFilter: 'high',
    searchFilter: 'release',
  })
  expect(readTaskPreferences('workspace-b').sort).toBe('manual')
})

test('invalid saved preferences use safe defaults', () => {
  localStorage.setItem('orbit:task_preferences:workspace-a', JSON.stringify({
    sort: 'unknown', statusFilter: 42, unassignedFilter: 'true', priorityFilter: 'critical', searchFilter: false,
  }))
  expect(readTaskPreferences('workspace-a')).toEqual({
    sort: 'manual', statusFilter: null, assigneeFilter: null, unassignedFilter: false,
    labelFilter: null, priorityFilter: null, searchFilter: '',
  })
  localStorage.setItem('orbit:task_preferences:workspace-a', '{broken')
  expect(readTaskPreferences('workspace-a').sort).toBe('manual')
})
