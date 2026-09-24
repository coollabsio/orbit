import { useEffect, useState } from 'react'
import type { TaskPriority } from './api/models'
import { SORT_OPTIONS, type SortKey } from './tasksLib'

export interface TaskPreferences {
  sort: SortKey
  statusFilter: string | null
  assigneeFilter: string | null
  unassignedFilter: boolean
  labelFilter: string | null
  priorityFilter: TaskPriority | null
  searchFilter: string
}

const defaults: TaskPreferences = {
  sort: 'manual',
  statusFilter: null,
  assigneeFilter: null,
  unassignedFilter: false,
  labelFilter: null,
  priorityFilter: null,
  searchFilter: '',
}

const key = (workspaceId: string) => `orbit:task_preferences:${workspaceId}`
const optionalString = (value: unknown) => typeof value === 'string' ? value : null

export function readTaskPreferences(workspaceId: string): TaskPreferences {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key(workspaceId)) ?? 'null')
    if (!value || typeof value !== 'object' || Array.isArray(value)) return { ...defaults }
    const saved = value as Record<string, unknown>
    return {
      sort: SORT_OPTIONS.some((option) => option.key === saved.sort) ? saved.sort as SortKey : defaults.sort,
      statusFilter: optionalString(saved.statusFilter),
      assigneeFilter: optionalString(saved.assigneeFilter),
      unassignedFilter: saved.unassignedFilter === true,
      labelFilter: optionalString(saved.labelFilter),
      priorityFilter: ['none', 'low', 'medium', 'high', 'urgent'].includes(String(saved.priorityFilter)) ? saved.priorityFilter as TaskPriority : null,
      searchFilter: typeof saved.searchFilter === 'string' ? saved.searchFilter : '',
    }
  } catch {
    return { ...defaults }
  }
}

export function useTaskPreferences(workspaceId: string) {
  const [preferences, setPreferences] = useState(() => readTaskPreferences(workspaceId))
  useEffect(() => {
    localStorage.setItem(key(workspaceId), JSON.stringify(preferences))
  }, [workspaceId, preferences])
  return [preferences, setPreferences] as const
}
