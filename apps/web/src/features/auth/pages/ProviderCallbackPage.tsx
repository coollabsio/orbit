import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { ApiProblem } from '@orbit/api/problem'
import { useProviderExchange } from '@/features/auth/api'
import { AuthCard } from '@/features/auth/components/AuthForm'
import { AuthMessage } from '@/features/auth/components/AuthMessage'
import { TwoFactorStep } from '@/features/auth/components/TwoFactorStep'
import { oauthLoginDestination } from '@/features/auth/oauth'
import { clearProviderFlow, parseProviderCallback, PROVIDER_NAMES, storedProviderFlow } from '@/features/auth/providerSignIn'

/** Where a `connect` or `confirm` goes back to: the account page that started it. */
function settingsDestination(returnTo: string | null) {
  return returnTo === '/profile' || returnTo?.startsWith('/profile/') ? returnTo : '/profile/security'
}

/**
 * `/login/callback#handoff=…`: the server's redirect back from Google or GitHub. Trades the handoff and this tab's
 * verifier for a session (or a code step), or finishes a `connect` / `confirm` for the signed-in account.
 */
export function ProviderCallbackPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const exchange = useProviderExchange()
  const [callback] = useState(() => ({ ...parseProviderCallback(location.hash), flow: storedProviderFlow() }))
  const [codeStepError, setCodeStepError] = useState<Error | null>(null)
  const started = useRef(false)
  const { handoff, returnTo, flow } = callback

  useEffect(() => {
    if (started.current) return
    started.current = true
    // The handoff is single-use, but it still should not stay in the history.
    void navigate(location.pathname, { replace: true })
    clearProviderFlow()
    if (!handoff || !flow) return
    exchange.mutate({ handoff, code_verifier: flow.verifier }, {
      onSuccess: (result) => {
        if (result.kind === 'signed_in') void navigate(oauthLoginDestination(returnTo), { replace: true })
        if (result.kind === 'done') {
          toast(flow.intent === 'connect' ? `${PROVIDER_NAMES[flow.provider]} is connected` : `Confirmed with ${PROVIDER_NAMES[flow.provider]}. Try the change again.`)
          void navigate(settingsDestination(returnTo), { replace: true })
        }
      },
    })
  }, [exchange, flow, handoff, location.pathname, navigate, returnTo])

  const signingIn = !flow || flow.intent === 'sign_in'
  const error = codeStepError ?? exchange.error
  if (error || !handoff || !flow) {
    const problem = error instanceof ApiProblem ? error : null
    return (
      <AuthCard>
        <h1 className="mb-1 text-xl font-semibold">{problem?.problem.title ?? 'Sign-in expired'}</h1>
        <p role="alert" className="text-[13px] leading-[1.6] text-muted-foreground">{problem ? problem.detail : error ? 'The server could not complete the sign-in. Try again.' : 'This sign-in has expired or was opened in another tab. Start again.'}</p>
        <p className="mt-4 text-[13px]">
          {signingIn
            ? <Link to="/login" className="font-medium text-foreground">Back to sign in</Link>
            : <Link to={settingsDestination(returnTo)} className="font-medium text-foreground">Back to settings</Link>}
        </p>
      </AuthCard>
    )
  }
  if (exchange.data?.kind === 'two_factor') {
    return <TwoFactorStep token={exchange.data.token} onSignedIn={() => void navigate(oauthLoginDestination(returnTo), { replace: true })} onBack={(reason) => {
      if (reason) setCodeStepError(reason)
      else void navigate('/login', { replace: true })
    }} />
  }
  return <AuthMessage title={signingIn ? 'Signing you in' : 'Confirming your account'} detail="One moment." />
}
