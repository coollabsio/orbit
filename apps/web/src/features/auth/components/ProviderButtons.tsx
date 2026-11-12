import type { Provider } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { ProviderMark } from '@/components/common/icons/ProviderMark'
import { Button } from '@/components/ui/button'
import { useProviderName, useStartProviderSignIn } from '@/features/auth/api'

/** "Continue with Google / GitHub / SSO" for the providers an admin configured. Signs in, or signs up where that is open. */
export function ProviderButtons({ providers, returnTo = null, disabled = false }: { providers: Provider[]; returnTo?: string | null; disabled?: boolean }) {
  const start = useStartProviderSignIn()
  const providerName = useProviderName()
  if (providers.length === 0) return null
  return (
    <>
      {providers.map((provider) => (
        // Stays disabled after success: the page is leaving for the provider.
        <Button key={provider} type="button" variant="outline" size="lg" disabled={disabled || start.isPending || start.isSuccess} onClick={() => start.mutate({ provider, intent: 'sign_in', returnTo })}>
          <ProviderMark provider={provider} />
          Continue with {providerName(provider)}
        </Button>
      ))}
      {start.isError ? <p role="alert" className="text-[0.8125rem] text-destructive">{start.error instanceof ApiProblem ? start.error.detail : 'The sign-in could not start. Try again.'}</p> : null}
    </>
  )
}
