import { useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { ApiProblem } from '@orbit/api/problem'
import { cn } from 'cn'
import { buttonVariants } from '@/components/ui/button'
import { useAppPasskeySignIn } from '@/features/auth/api'
import { AuthCard, AuthForm } from '@/features/auth/components/AuthForm'
import { AuthMessage } from '@/features/auth/components/AuthMessage'
import { isPasskeyCancel, passkeysSupported } from '@/features/auth/passkey'

/**
 * `/login/app#flow=…`: the Orbit app opens this page in the system browser to sign in with a passkey, because a passkey
 * belongs to this server's web origin. The passkey is always asked for, and only after a click: a browser that is
 * already signed in does not finish the flow by itself. Nobody is signed in here. The page ends at the app's redirect
 * with a one-time handoff, which only the app that started the flow can trade for a session.
 */
export function AppSignInPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const [flow] = useState(() => new URLSearchParams(location.hash.slice(1)).get('flow'))
  const signIn = useAppPasskeySignIn()
  const redirectUrl = signIn.data

  useEffect(() => {
    // The flow token should not stay in the address or the history.
    if (location.hash) void navigate(location.pathname, { replace: true })
  }, [location.hash, location.pathname, navigate])

  useEffect(() => {
    if (redirectUrl) window.location.assign(redirectUrl)
  }, [redirectUrl])

  const expired = signIn.error instanceof ApiProblem && signIn.error.code === 'invalid_app_sign_in'
  if (!flow || expired) return <AuthMessage title="Sign-in expired" detail="This sign-in has expired or was already used. Go back to the Orbit app and start again." />
  if (!passkeysSupported()) return <AuthMessage title="Passkeys are not available" detail="This browser does not support passkeys. Go back to the Orbit app and sign in another way." />
  if (redirectUrl) {
    return (
      <AuthCard>
        <h1 className="mb-1 text-xl font-semibold">Go back to the Orbit app</h1>
        <p className="text-[0.8125rem] leading-[1.6] text-muted-foreground">Your passkey is confirmed. If the app does not open, use the button.</p>
        {/* a link, not a script: a browser can refuse a redirect to an app that no click started */}
        <a href={redirectUrl} className={cn(buttonVariants({ size: 'lg' }), 'mt-4 w-full')}>Open the Orbit app</a>
      </AuthCard>
    )
  }
  return (
    <AuthForm
      title="Sign in to the Orbit app"
      description="Use your passkey to sign in to the Orbit app on this device. Continue only if you started this sign-in in the Orbit app."
      error={signIn.error && !isPasskeyCancel(signIn.error) ? signIn.error : null}
      pending={signIn.isPending}
      submitLabel="Sign in with a passkey"
      onSubmit={() => signIn.mutateAsync(flow)}
    >
      {null}
    </AuthForm>
  )
}
