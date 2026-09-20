import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useCompleteRecovery } from '@/features/auth/api'
import { recoveryRequestCopy } from '@/features/auth/authState'
import { AuthForm, AuthInput } from '@/features/auth/components/AuthForm'
import { AuthMessage } from '@/features/auth/components/AuthMessage'
import { useConsumedToken } from '@/features/auth/useConsumedToken'

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
