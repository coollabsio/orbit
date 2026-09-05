import { createContext, useContext } from 'react'
import type { WorkspaceRecord } from '../../api/generated/types.gen'

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
