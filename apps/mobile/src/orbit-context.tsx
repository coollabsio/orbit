import { useQuery } from '@tanstack/react-query'
import { createContext, type ReactNode, use, useEffect, useState } from 'react'

import { accountKeys, authOptions } from './data/account-api'
import { billingKey, workspaceBilling } from './data/billing'
import { keys } from './data/keys'
import { type OrbitClient, type User, type WorkspaceRecord, workspaces as listWorkspaces } from './data/orbit'
import { loadWorkspaceId, saveWorkspaceId, type StoredSession } from './session'
import { useSession } from './session-context'
import { Button, EmptyState, ErrorView, LoadingView, Screen } from './ui'

export interface Orbit {
  client: OrbitClient
  session: StoredSession
  user: User
  workspace: WorkspaceRecord
  workspaceId: string
  workspaces: WorkspaceRecord[]
  selectWorkspace: (id: string) => void
  signOut: () => Promise<void>
  setUser: (user: User) => void
}

const OrbitContext = createContext<Orbit | null>(null)

/**
 * The signed-in session and its current workspace: the one the user chose last, else the first. Renders its children
 * only once both are known, so every screen below can rely on `useOrbit()`.
 */
export function OrbitProvider({ children }: { children: ReactNode }) {
  const { state, signOut, setUser } = useSession()
  if (state.status !== 'signed_in') throw new Error('OrbitProvider needs a signed-in session')
  const { client, session, user } = state
  const list = useQuery({ queryKey: keys.workspaces, queryFn: () => listWorkspaces(client) })
  /** `undefined` while the stored choice loads. */
  const [chosen, setChosen] = useState<string | null | undefined>(undefined)

  useEffect(() => {
    void loadWorkspaceId().then(setChosen, () => setChosen(null))
  }, [])

  if (!list.data) {
    if (list.isError) return <ErrorView error={list.error} onRetry={() => void list.refetch()} />
    return <LoadingView />
  }
  if (chosen === undefined) return <LoadingView />

  const live = list.data.filter((workspace) => !workspace.deleted_at)
  const workspace = live.find((item) => item.id === chosen) ?? live[0]
  if (!workspace) {
    return (
      <Screen centered>
        <EmptyState icon="home" title="No workspaces" description="Ask an admin to invite you, or create a workspace in Orbit on the web." action={<Button label="Sign out" variant="outline" onPress={() => void signOut()} />} />
      </Screen>
    )
  }

  const selectWorkspace = (id: string) => {
    setChosen(id)
    void saveWorkspaceId(id)
  }

  return (
    <OrbitContext value={{ client, session, user, workspace, workspaceId: workspace.id, workspaces: live, selectWorkspace, signOut, setUser }}><BillingReady>{children}</BillingReady></OrbitContext>
  )
}

/**
 * Orbit Cloud: the first screen waits for the billing state, so the trial or read-only line above the dock is there
 * from the start and does not push the screen up when it arrives. A failed request does not hold the app.
 */
function BillingReady({ children }: { children: ReactNode }) {
  const { client, workspaceId } = useOrbit()
  const options = useQuery({ queryKey: accountKeys.authOptions, queryFn: () => authOptions(client), staleTime: Infinity })
  const enabled = options.data?.billing_enabled ?? false
  const billing = useQuery({ queryKey: billingKey(workspaceId), queryFn: () => workspaceBilling(client, workspaceId), enabled })
  const [ready, setReady] = useState(false)
  if (!ready && options.isFetched && (!enabled || billing.isFetched)) setReady(true)
  return ready ? children : <LoadingView />
}

export function useOrbit(): Orbit {
  const value = use(OrbitContext)
  if (!value) throw new Error('useOrbit must be used inside <OrbitProvider>')
  return value
}
