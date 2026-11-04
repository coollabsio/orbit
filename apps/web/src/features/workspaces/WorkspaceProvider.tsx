import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router'
import type { WorkspaceRecord } from '@orbit/api/generated/types.gen'
import { useWorkspaces } from './api'
import { useWorkspaceBilling } from './billing'
import { selectedWorkspaceId, switchWorkspaceHref } from './navigation'
import { useIsInstallationAdmin } from './permissions'
import { WorkspaceContext } from './workspaceContext'
import { OrbitUnavailable } from '@/components/common/OrbitUnavailable'
import { LoadingScreen } from '@/components/common/LoadingScreen'
import { firstLoadFailed } from '@/lib/connection'
import { useAuthOptions } from '@/features/auth/api'
const preferenceKey = 'orbit:selected_workspace'
const emptyWorkspaces: WorkspaceRecord[] = []
/** What an operator with no workspace can open: Admin and their own account settings. */
const operatorPages = /^\/(admin|profile)(\/|$)/

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const query = useWorkspaces()
  const location = useLocation()
  const navigate = useNavigate()
  const isOperator = useIsInstallationAdmin()
  const workspaces = query.data ?? emptyWorkspaces
  const workspaceId = selectedWorkspaceId(
    workspaces,
    location.search,
    window.localStorage.getItem(preferenceKey),
  )
  const workspace = workspaces.find((item) => item.id === workspaceId)

  useEffect(() => {
    if (workspaceId) window.localStorage.setItem(preferenceKey, workspaceId)
  }, [workspaceId])

  const value = useMemo(() => workspace ? {
    workspace,
    workspaces,
    selectWorkspace: (id: string) => navigate(switchWorkspaceHref(location.pathname, location.search, id)),
  } : null, [location.pathname, location.search, navigate, workspace, workspaces])

  // With workspaces from before, a refetch that lost the connection keeps the app on screen (the banner in the shell
  // tells the user); one the server refused does not.
  if (firstLoadFailed(query)) return <OrbitUnavailable detail="Your workspaces could not be loaded." onRetry={() => void query.refetch()} />
  if (query.data === undefined) return <LoadingScreen />
  if (!value) {
    // Operators (the root user, instance admins) are not customers: with no workspace they land in Admin, without
    // a workspace context (`AppShell` shows the operator frame), and may still create one there.
    if (isOperator) return operatorPages.test(location.pathname) ? children : <Navigate to="/admin" replace />
    // Anyone else with no workspace (the last one is gone, or they left it) creates one.
    return <Navigate to="/onboarding" replace />
  }
  return <WorkspaceContext.Provider value={value}><BillingReady>{children}</BillingReady></WorkspaceContext.Provider>
}

/**
 * Orbit Cloud: the first screen waits for the billing state, so the trial or read-only line above the top bar is
 * there from the start and does not push the page down when it arrives. A failed request does not hold the app.
 */
function BillingReady({ children }: { children: ReactNode }) {
  const options = useAuthOptions()
  const billing = useWorkspaceBilling()
  const [ready, setReady] = useState(false)
  if (!ready && options.isFetched && (!options.data?.billing_enabled || billing.isFetched)) setReady(true)
  return ready ? children : <LoadingScreen />
}
