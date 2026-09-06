import { useState } from 'react'
import { Link, Navigate, Outlet, useNavigate } from 'react-router'
import { ApiProblem } from '../../api/problem'
import { useAcceptInvitation } from '../workspaces/api'
import { authGateState, initialLoginValues, recoveryRequestCopy } from './authState'
import {
  useCompleteRecovery,
  useCompleteSetup,
  useCurrentUser,
  useLogin,
  useSetupStatus,
} from './api'
import { consumeQueryToken } from './authFlow'

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
  return <AuthMessage title="Loading Orbit" detail="Checking the installation and session." />
}

function useConsumedToken() {
  const [token] = useState(() => consumeQueryToken(window.location.href, (href) => window.history.replaceState(null, '', href)))
  return token
}

export function SetupPage() {
  const navigate = useNavigate()
  const setup = useCompleteSetup()
  const token = useConsumedToken()
  const [form, setForm] = useState({ display_name: '', email: '', password: '', workspace_name: '', project_name: '' })
  if (!token) return <AuthMessage title="Set up Orbit" detail="Open the one-time setup link printed by the server." />
  return (
    <AuthForm title="Set up Orbit" error={setup.error} pending={setup.isPending} submitLabel="Create workspace" onSubmit={async () => {
      await setup.mutateAsync({ token, ...form })
      navigate('/', { replace: true })
    }}>
      <AuthInput label="Your name" value={form.display_name} onChange={(display_name) => setForm({ ...form, display_name })} />
      <AuthInput label="Email" type="email" value={form.email} onChange={(email) => setForm({ ...form, email })} />
      <AuthInput label="Password" type="password" value={form.password} onChange={(password) => setForm({ ...form, password })} />
      <AuthInput label="Workspace name" value={form.workspace_name} onChange={(workspace_name) => setForm({ ...form, workspace_name })} />
      <AuthInput label="First project" value={form.project_name} onChange={(project_name) => setForm({ ...form, project_name })} />
    </AuthForm>
  )
}

export function LoginPage() {
  const navigate = useNavigate()
  const mutation = useLogin()
  const [email, setEmail] = useState(() => initialLoginValues(import.meta.env.DEV).email)
  const [password, setPassword] = useState(() => initialLoginValues(import.meta.env.DEV).password)
  return (
    <AuthForm title="Sign in to Orbit" error={mutation.error} pending={mutation.isPending} submitLabel="Sign in" onSubmit={async () => {
      await mutation.mutateAsync({ email, password })
      navigate('/', { replace: true })
    }} footer={<Link to="/recovery">Forgot your password?</Link>}>
      <AuthInput label="Email" type="email" value={email} onChange={setEmail} />
      <AuthInput label="Password" type="password" value={password} onChange={setPassword} />
    </AuthForm>
  )
}

export function RecoveryPage() {
  const token = useConsumedToken()
  return token ? <CompleteRecovery token={token} /> : <AuthMessage {...recoveryRequestCopy} />
}

function CompleteRecovery({ token }: { token: string }) {
  const navigate = useNavigate()
  const mutation = useCompleteRecovery()
  const [password, setPassword] = useState('')
  return (
    <AuthForm title="Choose a new password" error={mutation.error} pending={mutation.isPending} submitLabel="Reset password" onSubmit={async () => {
      await mutation.mutateAsync({ token, password })
      navigate('/login', { replace: true })
    }}>
      <AuthInput label="New password" type="password" value={password} onChange={setPassword} />
    </AuthForm>
  )
}

export function AcceptInvitationPage() {
  const navigate = useNavigate()
  const token = useConsumedToken()
  const mutation = useAcceptInvitation()
  const [email, setEmail] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  if (!token) return <AuthMessage title="Invitation unavailable" detail="This invitation link is missing its token." />
  return (
    <AuthForm title="Join this workspace" error={mutation.error} pending={mutation.isPending} submitLabel="Accept invitation" onSubmit={async () => {
      await mutation.mutateAsync({ token, email: email || undefined, display_name: displayName || undefined, password: password || undefined })
      navigate('/', { replace: true })
    }}>
      <p className="auth-boundary-hint">Already signed in? Submit without filling the optional account fields.</p>
      <AuthInput label="Name (new accounts)" required={false} value={displayName} onChange={setDisplayName} />
      <AuthInput label="Email (new accounts)" required={false} type="email" value={email} onChange={setEmail} />
      <AuthInput label="Password (new accounts)" required={false} type="password" value={password} onChange={setPassword} />
    </AuthForm>
  )
}

function AuthInput({ label, value, onChange, type = 'text', required = true }: { label: string; value: string; onChange: (value: string) => void; type?: string; required?: boolean }) {
  return <label className="auth-boundary-field"><span>{label}</span><input className="input" type={type} required={required} value={value} onChange={(event) => onChange(event.target.value)} /></label>
}

function AuthForm({ title, children, error, pending, submitLabel, onSubmit, footer }: { title: string; children: React.ReactNode; error: Error | null; pending: boolean; submitLabel: string; onSubmit: () => Promise<unknown>; footer?: React.ReactNode }) {
  return (
    <main className="auth-boundary-page"><form className="auth-boundary-card auth-boundary-form" onSubmit={(event) => { event.preventDefault(); void onSubmit().catch(() => undefined) }}>
      <span className="auth-boundary-wordmark">Orbit</span><h1>{title}</h1>
      <div className="auth-boundary-fields">{children}</div>
      {error ? <p className="auth-boundary-error" role="alert">{error instanceof ApiProblem ? error.detail : 'The server could not complete the request.'}</p> : null}
      <button className="button button-primary" type="submit" disabled={pending}>{pending ? 'Please wait…' : submitLabel}</button>
      {footer ? <div className="auth-boundary-footer">{footer}</div> : null}
    </form></main>
  )
}

function AuthMessage({ title, detail }: { title: string; detail: string }) {
  return <main className="auth-boundary-page"><section className="auth-boundary-card"><span className="auth-boundary-wordmark">Orbit</span><h1>{title}</h1><p>{detail}</p></section></main>
}
