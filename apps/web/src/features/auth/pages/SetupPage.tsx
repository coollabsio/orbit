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
  const [form, setForm] = useState({ display_name: '', email: '', password: '' })
  if (!token) return <AuthMessage title="Set up Orbit" detail="Open the one-time setup link printed by the server. The account you create there becomes the root user of this instance." />
  return (
    <AuthForm title="Create the root account" error={setup.error} pending={setup.isPending} submitLabel="Create root account" onSubmit={async () => {
      await setup.mutateAsync({ token, ...form })
      navigate('/onboarding', { replace: true })
    }}>
      <div role="note" className="rounded-lg border border-primary/30 bg-primary/5 p-3 text-[0.8125rem] text-muted-foreground">
        <p><strong className="font-semibold text-foreground">This account becomes the root user.</strong> The root user manages this Orbit instance in Admin (accounts, email, backups, audit log) and can choose other admins to help. Nobody else can suspend or reset the root account.</p>
        <p className="mt-1.5">Use a strong password and keep it safe. You create your first workspace in the next step.</p>
      </div>
      <AuthInput label="Your name" value={form.display_name} onChange={(display_name) => setForm({ ...form, display_name })} />
      <AuthInput label="Email" type="email" value={form.email} onChange={(email) => setForm({ ...form, email })} />
      <AuthInput label="Password" type="password" value={form.password} onChange={(password) => setForm({ ...form, password })} />
    </AuthForm>
  )
}
