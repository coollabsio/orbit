import { useState } from 'react'
import type { Provider } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { useLinkProviderSignIn } from '@/features/auth/api'
import { AuthForm, AuthInput } from '@/features/auth/components/AuthForm'
import { PROVIDER_NAMES } from '@/features/auth/providerSignIn'

/**
 * The password step when a Google or GitHub account first connects to an existing Orbit account by its email.
 * `onTwoFactor` gets the code challenge when the account has two-factor sign-in on. `onBack` gets the reason when
 * the step expired or was used up, and `null` when the person pressed Back.
 */
export function LinkPasswordStep({ token, provider, email, onSignedIn, onTwoFactor, onBack }: {
  token: string
  provider: Provider
  email: string
  onSignedIn: () => void
  onTwoFactor: (token: string) => void
  onBack: (error: Error | null) => void
}) {
  const link = useLinkProviderSignIn()
  const [password, setPassword] = useState('')
  const name = PROVIDER_NAMES[provider]
  return (
    <AuthForm
      title={`Connect ${name}`}
      description={`An Orbit account for ${email} already exists. Enter its password to connect your ${name} account. You only do this once.`}
      error={link.error}
      pending={link.isPending}
      submitLabel="Connect and sign in"
      onSubmit={async () => {
        try {
          const result = await link.mutateAsync({ password_token: token, password })
          if ('two_factor_token' in result) onTwoFactor(result.two_factor_token)
          else onSignedIn()
        } catch (error) {
          if (error instanceof ApiProblem && error.code === 'invalid_password_token') onBack(error)
          throw error
        }
      }}
      footer={<Button type="button" variant="link" className="h-auto p-0" onClick={() => onBack(null)}>Back</Button>}
    >
      <AuthInput label="Email" value={email} />
      <AuthInput label="Password" type="password" value={password} onChange={setPassword} />
    </AuthForm>
  )
}
