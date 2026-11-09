import { createContext, useContext } from 'react'
import type { ViewContext } from '@orbit/database/types'
import type { User } from '@/features/workspaces/models'

/** What every part of a database page reads: the viewer, the members and what the viewer may change. */
export interface DatabaseEnv {
  workspaceId: string
  databaseId: string
  ctx: ViewContext
  users: ReadonlyMap<string, User>
  /** Rows and values (a read-only workspace refuses them). */
  canEditValues: boolean
  /** Properties and views (also refused while the database is locked). */
  canEditSchema: boolean
}

export const DatabaseEnvContext = createContext<DatabaseEnv | null>(null)

export function useDatabaseEnv(): DatabaseEnv {
  const env = useContext(DatabaseEnvContext)
  if (!env) throw new Error('useDatabaseEnv needs a DatabaseEnvContext provider.')
  return env
}
