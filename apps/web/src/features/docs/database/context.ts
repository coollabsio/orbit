import { createContext, useContext, useState } from 'react'
import type { ViewContext } from '@orbit/database/types'
import { useCurrentUser } from '@/features/auth/api'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspaceReadOnly } from '@/features/workspaces/billing'
import type { User } from '@/features/workspaces/models'

/** What every part of a database page reads: the viewer, the members and what the viewer may change. */
export interface DatabaseEnv {
  workspaceId: string
  databaseId: string
  ctx: ViewContext
  users: ReadonlyMap<string, User>
  /** Rows and values (a read-only workspace refuses them). */
  canEditValues: boolean
  /** Properties (also refused while the database is locked). */
  canEditSchema: boolean
  /** The views shown: the database's own follow its lock, a linked view block's follow its host page's lock. */
  canEditViews: boolean
  /** The views shown are a linked view block's (`database_links` id). */
  linkId: string | null
  /** Opens a row of this database in the side peek (a relation pill); absent outside a database surface. */
  openRow?: (rowId: string) => void
}

export const DatabaseEnvContext = createContext<DatabaseEnv | null>(null)

export function useDatabaseEnv(): DatabaseEnv {
  const env = useContext(DatabaseEnvContext)
  if (!env) throw new Error('useDatabaseEnv needs a DatabaseEnvContext provider.')
  return env
}

/**
 * The env of a database for the viewer: `locked` is the database page's lock. `link`: the views shown are a linked
 * view block's, frozen by its host page's lock instead.
 */
export function useDatabaseEnvValue(workspaceId: string, databaseId: string, locked: boolean, link?: { id: string; hostLocked: boolean }): DatabaseEnv {
  const membersQuery = useMembers(workspaceId)
  const me = useCurrentUser()
  const workspaceReadOnly = useWorkspaceReadOnly()
  const [now] = useState(() => Date.now())
  const [timeZone] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone)
  const members = membersQuery.data ?? []
  return {
    workspaceId,
    databaseId,
    ctx: { userId: me.data?.id ?? '', now, timeZone, members: new Map(members.map((user) => [user.id, user.name])) },
    users: new Map(members.map((user) => [user.id, user])),
    canEditValues: !workspaceReadOnly,
    canEditSchema: !locked && !workspaceReadOnly,
    canEditViews: !(link ? link.hostLocked : locked) && !workspaceReadOnly,
    linkId: link?.id ?? null,
  }
}
