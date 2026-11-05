import { useQuery } from '@tanstack/react-query'
import { Navigate, Outlet, useLocation, useSearchParams } from 'react-router'
import { createApiClient } from '@/api/client'
import { LoadingScreen } from '@/components/common/LoadingScreen'
import { firstLoadFailed } from '@/lib/connection'
import { authGateState } from './authState'
import { currentUserQueryOptions, useCurrentUser, useSetupStatus } from './api'
import { loginDestination, loginPath } from './returnTo'
import { OrbitUnavailable } from '@/components/common/OrbitUnavailable'

export function AuthGate() {
  const location = useLocation()
  const setup = useSetupStatus()
  const user = useCurrentUser(setup.data?.complete === true)

  const state = authGateState({
    setupComplete: setup.data?.complete,
    user: user.data,
    failed: firstLoadFailed(setup) || firstLoadFailed(user),
  })
  if (state === 'unavailable') {
    return <OrbitUnavailable detail="The server could not verify this session." onRetry={() => { void setup.refetch(); void user.refetch() }} />
  }
  if (state === 'setup') return <Navigate to="/setup" replace />
  if (state === 'login') return <Navigate to={loginPath(`${location.pathname}${location.search}${location.hash}`)} replace />
  if (state === 'authenticated') return <Outlet />
  return <LoadingScreen />
}

// Signed-out visitors are expected here; a 401 must not trigger the global login redirect.
const guestSessionClient = createApiClient()

/** Sends a signed-in user away from the sign-in pages (login, register, recovery, setup) into the app. */
export function GuestGate() {
  const [params] = useSearchParams()
  const user = useQuery({ ...currentUserQueryOptions(guestSessionClient), retry: false })
  if (user.data) return <Navigate to={loginDestination(params.get('return_to'))} replace />
  if (user.isPending) return <LoadingScreen />
  return <Outlet />
}
