import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useCompleteSetup } from '@/features/auth/api'
import { AuthForm, AuthInput } from '@/features/auth/components/AuthForm'
import { AuthMessage } from '@/features/auth/components/AuthMessage'
import { useConsumedToken } from '@/features/auth/useConsumedToken'

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
