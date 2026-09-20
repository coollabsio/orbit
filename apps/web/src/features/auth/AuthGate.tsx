import { Navigate, Outlet } from 'react-router'
import { LoadingScreen } from '@/components/common/LoadingScreen'
import { authGateState } from './authState'
import { useCurrentUser, useSetupStatus } from './api'
import { AuthMessage } from '@/features/auth/components/AuthMessage'

export function AuthGate() {
  const setup = useSetupStatus()
  const user = useCurrentUser(setup.data?.complete === true)

  if (setup.isError || user.isError) {
    return <AuthMessage title="Orbit is unavailable" detail="The server could not verify this session." />
  }
  const state = authGateState({
    setupComplete: setup.data?.complete,
    user: user.data,
    userStatus: user.status,
  })
  if (state === 'setup') return <Navigate to="/setup" replace />
  if (state === 'login') return <Navigate to="/login" replace />
  if (state === 'authenticated') return <Outlet />
  return <LoadingScreen />
}
