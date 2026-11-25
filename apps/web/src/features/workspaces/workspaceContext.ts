import { createContext, useContext } from 'react'
import type { OptionalTaskProperty, WorkspaceRecord } from '@orbit/api/generated/types.gen'
import { showsTaskProperty } from '@orbit/api/taskProperties'

export interface WorkspaceContextValue {
  workspace: WorkspaceRecord
  workspaces: WorkspaceRecord[]
  selectWorkspace: (workspaceId: string) => void
}

export const WorkspaceContext = createContext<WorkspaceContextValue | null>(null)

export function useWorkspace(): WorkspaceContextValue {
  const value = useContext(WorkspaceContext)
  if (!value) throw new Error('useWorkspace must be used within WorkspaceProvider')
  return value
}

/** Whether the current workspace shows the built-in task property (Settings, Task fields). */
export function useShowsTaskProperty(property: OptionalTaskProperty): boolean {
  return showsTaskProperty(useWorkspace().workspace, property)
}
