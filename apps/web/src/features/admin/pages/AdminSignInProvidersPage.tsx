import { useId, useState } from 'react'
import { toast } from 'sonner'
import { Copy } from 'reicon-react'
import type { Provider, SignInProviderView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { useInstanceSettings, useRemoveSignInProvider, useSaveSignInProvider } from '@/features/admin/api'
import { PROVIDER_NAMES } from '@/features/auth/providerSignIn'
import { FieldGrid, RequiredMark } from '@/features/settings/components/SettingsParts'

const PROVIDERS: { provider: Provider; description: string }[] = [
  { provider: 'google', description: 'Create a client of type "Web application" in Google Cloud Console (APIs & Services → Credentials) and add the callback URL as an authorized redirect URI.' },
  { provider: 'github', description: 'Register an app in GitHub (Settings → Developer settings → OAuth Apps) with the callback URL as its authorization callback URL.' },
]

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** "Continue with Google / GitHub" on the sign-in page: the client each provider issued to this Orbit. */
export function AdminSignInProvidersPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="Sign-in providers"><p role="status">Loading settings…</p></SettingsCard>
  if (settings.isError) return <SettingsCard title="Sign-in providers"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  return (
    <>
      {PROVIDERS.map(({ provider, description }) => {
        const saved = settings.data.sign_in_providers.find((item) => item.provider === provider) ?? null
        // Remount on save so the form shows what the server stored.
        return <ProviderCard key={`${provider}:${JSON.stringify(saved)}`} provider={provider} description={description} saved={saved} />
      })}
    </>
  )
}

function ProviderCard({ provider, description, saved }: { provider: Provider; description: string; saved: SignInProviderView | null }) {
  const save = useSaveSignInProvider()
  const remove = useRemoveSignInProvider()
  const [clientId, setClientId] = useState(saved?.client_id ?? '')
  const [secret, setSecret] = useState('')
  const id = useId()
  const name = PROVIDER_NAMES[provider]
  // The server reports the callback URL of saved providers only; before the first save it is this origin's.
  const callbackUrl = saved?.callback_url ?? `${window.location.origin}/api/v1/auth/providers/${provider}/callback`

  async function removeProvider() {
    const confirmed = await confirmAction({
      title: `Remove ${name}?`,
      description: `"Continue with ${name}" disappears from the sign-in page. Accounts that only sign in with ${name} need a password reset to get back in.`,
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(provider, {
      onSuccess: () => toast(`${name} removed`),
      onError: (error) => toast.error(problemText(error, `${name} could not be removed.`)),
    })
  }

  return (
    <form onSubmit={(event) => {
      event.preventDefault()
      save.mutate({ provider, body: { client_id: clientId.trim(), client_secret: secret || null } }, {
        onSuccess: () => toast(`${name} saved`),
        onError: (error) => toast.error(problemText(error, `${name} could not be saved.`)),
      })
    }}>
      <SettingsCard title={name} description={description}>
        <FieldGrid>
          <Field className="min-[900px]:col-span-2">
            <FieldLabel htmlFor={`${id}-callback`}>Callback URL</FieldLabel>
            <div className="flex items-center gap-2">
              <Input id={`${id}-callback`} readOnly value={callbackUrl} className="font-mono" onFocus={(event) => event.target.select()} />
              <Button type="button" variant="outline" size="icon" aria-label="Copy callback URL" onClick={() => {
                void navigator.clipboard.writeText(callbackUrl).then(() => toast('Copied'), () => toast.error('Could not copy. Select and copy it manually.'))
              }}>
                <Copy />
              </Button>
            </div>
          </Field>
          <Field>
            <FieldLabel htmlFor={`${id}-client-id`}>Client ID <RequiredMark /></FieldLabel>
            <Input id={`${id}-client-id`} required autoComplete="off" spellCheck={false} value={clientId} onChange={(event) => setClientId(event.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor={`${id}-secret`}>Client secret {saved?.secret_set ? null : <RequiredMark />}</FieldLabel>
            <PasswordInput id={`${id}-secret`} autoComplete="new-password" required={!saved?.secret_set} placeholder={saved?.secret_set ? 'Saved. Leave empty to keep it.' : ''} value={secret} onChange={(event) => setSecret(event.target.value)} />
            <FieldDescription>Stored encrypted.</FieldDescription>
          </Field>
        </FieldGrid>
        <div className="mt-4 flex items-center justify-end gap-2">
          {saved ? <Button type="button" variant="ghost" className="text-destructive" disabled={remove.isPending} onClick={() => void removeProvider()}>Remove</Button> : null}
          <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : `Save ${name}`}</Button>
        </div>
      </SettingsCard>
    </form>
  )
}
