import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { useLogin } from '@/features/auth/api'
import { initialLoginValues } from '@/features/auth/authState'
import { AuthForm, AuthInput } from '@/features/auth/components/AuthForm'

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
