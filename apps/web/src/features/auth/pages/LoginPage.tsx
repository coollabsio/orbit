import { useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { isTwoFactorChallenge, useAuthOptions, useLogin, usePasskeyLogin } from '@/features/auth/api'
import { initialLoginValues } from '@/features/auth/authState'
import { AuthForm, AuthInput } from '@/features/auth/components/AuthForm'
import { ProviderButtons } from '@/features/auth/components/ProviderButtons'
import { TwoFactorStep } from '@/features/auth/components/TwoFactorStep'
import { isPasskeyCancel, passkeysSupported } from '@/features/auth/passkey'

import { oauthLoginDestination } from '@/features/auth/oauth'

/** The server sends an expired or replayed provider sign-in back here with `?sign_in_error=provider_expired`. */
function signInError(code: string | null): Error | null {
  if (code !== 'provider_expired') return null
  return new ApiProblem({ type: 'about:blank', title: 'Sign-in expired', status: 400, code, detail: 'The sign-in took too long or was already used. Try again.', instance: '/login', request_id: '' })
}

export function LoginPage() {
  const [params] = useSearchParams()
  const navigate = useNavigate()
  const mutation = useLogin()
  const passkey = usePasskeyLogin()
  const options = useAuthOptions()
  const [email, setEmail] = useState(() => initialLoginValues(import.meta.env.DEV).email)
  const [password, setPassword] = useState(() => initialLoginValues(import.meta.env.DEV).password)
  // Set while the password step is done and the account asks for a code.
  const [twoFactorToken, setTwoFactorToken] = useState<string | null>(null)
  // Why the code step ended (expired or used-up challenge), or why a provider sign-in came back, shown on the password step.
  const [returnError, setReturnError] = useState<Error | null>(() => signInError(params.get('sign_in_error')))
  const signedIn = () => navigate(oauthLoginDestination(params.get('return_to')), { replace: true })

  if (twoFactorToken) {
    return <TwoFactorStep token={twoFactorToken} onSignedIn={() => void signedIn()} onBack={(error) => {
      setTwoFactorToken(null)
      setReturnError(error)
    }} />
  }

  const showPasskey = options.data?.passkeys_enabled === true && passkeysSupported()
  const passkeyError = passkey.error && !isPasskeyCancel(passkey.error) ? passkey.error : null
  return (
    <AuthForm title="Orbit" description="Sign in to your tasks, docs, mail and chat." error={mutation.error ?? passkeyError ?? returnError} pending={mutation.isPending || passkey.isPending} submitLabel="Sign in" onSubmit={async () => {
      setReturnError(null)
      passkey.reset()
      const response = await mutation.mutateAsync({ email, password })
      if (isTwoFactorChallenge(response)) {
        setTwoFactorToken(response.two_factor_token)
        return
      }
      void signedIn()
    }} actions={<>
      {showPasskey ? <Button type="button" variant="outline" size="lg" className="border-primary/30 bg-primary/10 hover:bg-primary/15 dark:border-primary/30 dark:bg-primary/10 dark:hover:bg-primary/15" disabled={mutation.isPending || passkey.isPending} onClick={() => {
        setReturnError(null)
        mutation.reset()
        passkey.mutate(undefined, { onSuccess: () => void signedIn() })
      }}>
        Sign in with a passkey
      </Button> : null}
      <ProviderButtons providers={options.data?.providers ?? []} returnTo={params.get('return_to')} disabled={mutation.isPending || passkey.isPending} />
    </>} footer={options.data?.registration_open || options.data?.provider_registration_open ? <>New to Orbit? <Link to="/register" className="font-medium text-foreground">Create an account</Link></> : undefined}>
      <AuthInput label="Email" type="email" value={email} onChange={setEmail} />
      <AuthInput label="Password" type="password" value={password} onChange={setPassword} />
      <Link to="/recovery" className="justify-self-end text-[13px] text-muted-foreground hover:text-foreground">Forgot password?</Link>
    </AuthForm>
  )
}
