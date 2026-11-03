import { ApiProblem } from '@orbit/api/problem'
import { ProviderMark } from '@/components/common/icons/ProviderMark'
import { Button } from '@/components/ui/button'
import { useConnectedAccounts, useStartProviderSignIn } from '@/features/auth/api'
import { PROVIDER_NAMES } from '@/features/auth/providerSignIn'

/**
 * Answers 401 `reauthentication_required` for an account without a password: a fresh sign-in with its connected
 * provider, which comes back to `returnTo`. The person then tries the change again.
 */
export function ConfirmWithProvider({ returnTo }: { returnTo: string }) {
  const accounts = useConnectedAccounts()
  const start = useStartProviderSignIn()
  const provider = accounts.data?.[0]?.provider
  if (!provider) return <p role="alert" className="text-[13px] text-destructive">Sign out and sign in again, then try again.</p>
  const name = PROVIDER_NAMES[provider]
  return (
    <div role="alert" className="grid gap-3 text-[13px]">
      <p>For your security, confirm it is you with {name} first. Then try again.</p>
      {start.isError ? <p className="text-destructive">{start.error instanceof ApiProblem ? start.error.detail : 'The confirmation could not start.'}</p> : null}
      <Button type="button" variant="outline" className="justify-self-start" disabled={start.isPending || start.isSuccess} onClick={() => start.mutate({ provider, intent: 'confirm', returnTo })}>
        <ProviderMark provider={provider} />
        Confirm with {name}
      </Button>
    </div>
  )
}
