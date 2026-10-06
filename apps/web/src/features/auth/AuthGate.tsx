import { Navigate, Outlet } from 'react-router'
import { LoadingScreen } from '@/components/common/LoadingScreen'
import { firstLoadFailed } from '@/lib/connection'
import { authGateState } from './authState'
import { useCurrentUser, useSetupStatus } from './api'
import { OrbitUnavailable } from '@/components/common/OrbitUnavailable'

export function AuthGate() {
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
  if (state === 'login') return <Navigate to="/login" replace />
  if (state === 'authenticated') return <Outlet />
  return <LoadingScreen />
}
