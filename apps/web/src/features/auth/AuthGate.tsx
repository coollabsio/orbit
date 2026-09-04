import { useQuery } from '@tanstack/react-query'
import { Navigate, Outlet } from 'react-router'
import { apiClient } from '../../api/client'
import { me, setupStatus } from '../../api/generated/sdk.gen'
import { ApiProblem } from '../../api/problem'
import { authGateState } from './authState'
export function AuthGate() {
  if (import.meta.env.DEV && import.meta.env.VITE_API_MODE !== 'server') return <Outlet />
  return <ServerAuthGate />
}

function ServerAuthGate() {
  const setup = useQuery({
    queryKey: ['setup-status'],
    queryFn: async () => {
      const { data } = await setupStatus({ client: apiClient, throwOnError: true })
      if (!data) throw new Error('Setup status response was empty.')
      return data
    },
  })
  const user = useQuery({
    queryKey: ['current-user'],
    enabled: setup.data?.complete === true,
    queryFn: async () => {
      try {
        const { data } = await me({ client: apiClient, throwOnError: true })
        if (!data) throw new Error('Current user response was empty.')
        return data
      } catch (error) {
        if (error instanceof ApiProblem && error.status === 401) return null
        throw error
      }
    },
  })

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
  return <AuthMessage title="Loading Orbit" detail="Checking the installation and session." />
}

export function SetupPage() {
  return (
    <AuthMessage
      title="Set up Orbit"
      detail="Installation setup is ready. The complete setup form arrives with the workspace cutover."
    />
  )
}

export function LoginPage() {
  return (
    <AuthMessage
      title="Sign in to Orbit"
      detail="Authentication is ready. The complete sign-in form arrives with the workspace cutover."
    />
  )
}

export function RecoveryPage() {
  return (
    <AuthMessage
      title="Recover your account"
      detail="Password recovery is ready. The complete recovery form arrives with the workspace cutover."
    />
  )
}

function AuthMessage({ title, detail }: { title: string; detail: string }) {
  return (
    <main className="auth-boundary-page">
      <section className="auth-boundary-card">
        <span className="auth-boundary-wordmark">Orbit</span>
        <h1>{title}</h1>
        <p>{detail}</p>
      </section>
    </main>
  )
}
