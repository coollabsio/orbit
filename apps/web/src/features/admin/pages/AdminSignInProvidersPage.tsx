import { useId, useState } from 'react'
import { toast } from 'sonner'
import { Copy } from 'reicon-react'
import type { Provider, SignInProviderView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { InfoTip } from '@/components/common/InfoTip'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { useInstanceSettings, useRemoveSignInProvider, useSaveSignInProvider } from '@/features/admin/api'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'
import { useAuthOptions } from '@/features/auth/api'
import { PROVIDER_NAMES } from '@/features/auth/providerSignIn'
import { FieldGrid, RequiredMark } from '@/features/settings/components/SettingsParts'
import { LoadingText } from '@/components/ui/spinner'

/** The providers an admin can save here; OpenID Connect is set by environment variables only. */
type SavableProvider = Exclude<Provider, 'oidc'>

const PROVIDERS: { provider: SavableProvider; variable: string; description: string }[] = [
  { provider: 'google', variable: 'ORBIT_GOOGLE_*', description: 'Create a client of type "Web application" in Google Cloud Console (APIs & Services → Credentials) and add the callback URL as an authorized redirect URI.' },
  { provider: 'github', variable: 'ORBIT_GITHUB_*', description: 'Register an app in GitHub (Settings → Developer settings → OAuth Apps) with the callback URL as its authorization callback URL.' },
]

const OIDC_VARIABLES = ['ORBIT_OIDC_ISSUER', 'ORBIT_OIDC_CLIENT_ID', 'ORBIT_OIDC_CLIENT_SECRET', 'ORBIT_OIDC_DISPLAY_NAME']

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** The server reports the callback URL of saved providers only; before that it is this origin's. */
function callbackUrlOf(provider: Provider, saved: SignInProviderView | null) {
  return saved?.callback_url ?? `${window.location.origin}/api/v1/auth/providers/${provider}/callback`
}

/** "Continue with Google / GitHub / SSO" on the sign-in page: the client each provider issued to this Orbit. */
export function AdminSignInProvidersPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="Sign-in providers"><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (settings.isError) return <SettingsCard title="Sign-in providers"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  return (
    <>
      {PROVIDERS.map(({ provider, variable, description }) => {
        const saved = settings.data.sign_in_providers.find((item) => item.provider === provider) ?? null
        const lockedBy = settings.data.environment.sign_in_providers.includes(provider) ? variable : null
        // Remount on save so the form shows what the server stored.
        return <ProviderCard key={`${provider}:${JSON.stringify(saved)}`} provider={provider} description={description} saved={saved} lockedBy={lockedBy} />
      })}
      <OidcCard
        configured={settings.data.environment.sign_in_providers.includes('oidc')}
        saved={settings.data.sign_in_providers.find((item) => item.provider === 'oidc') ?? null}
      />
    </>
  )
}

/** `lockedBy`: the environment variables that set this provider. */
function ProviderCard({ provider, description, saved, lockedBy }: { provider: SavableProvider; description: string; saved: SignInProviderView | null; lockedBy: string | null }) {
  const locked = lockedBy != null
  const save = useSaveSignInProvider()
  const remove = useRemoveSignInProvider()
  const [clientId, setClientId] = useState(saved?.client_id ?? '')
  const [secret, setSecret] = useState('')
  const id = useId()
  const name = PROVIDER_NAMES[provider]

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
          <CallbackUrlField id={`${id}-callback`} url={callbackUrlOf(provider, saved)} />
          <Field>
            <FieldLabel htmlFor={`${id}-client-id`}>Client ID <RequiredMark /></FieldLabel>
            <Input id={`${id}-client-id`} required autoComplete="off" spellCheck={false} disabled={locked} value={clientId} onChange={(event) => setClientId(event.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor={`${id}-secret`}>Client secret {saved?.secret_set ? null : <RequiredMark />} <InfoTip text="Stored encrypted." /></FieldLabel>
            <PasswordInput id={`${id}-secret`} autoComplete="new-password" required={!saved?.secret_set} placeholder={saved?.secret_set ? 'Saved. Leave empty to keep it.' : ''} disabled={locked} value={secret} onChange={(event) => setSecret(event.target.value)} />
          </Field>
        </FieldGrid>
        <div className="mt-4 flex items-center justify-end gap-2">
          {lockedBy ? <SetByEnvironment variable={lockedBy} className="mr-auto" /> : null}
          {saved ? <Button type="button" variant="ghost" className="text-destructive" disabled={locked || remove.isPending} onClick={() => void removeProvider()}>Remove</Button> : null}
          <Button type="submit" disabled={locked || save.isPending}>{save.isPending ? 'Saving…' : `Save ${name}`}</Button>
        </div>
      </SettingsCard>
    </form>
  )
}

/**
 * A generic OpenID Connect provider (Authentik, Keycloak, …). Only `ORBIT_OIDC_*` sets it, so the card shows the
 * values without letting Admin change them.
 */
function OidcCard({ configured, saved }: { configured: boolean; saved: SignInProviderView | null }) {
  const displayName = useAuthOptions().data?.oidc_name ?? ''
  const id = useId()
  return (
    <SettingsCard
      title="OpenID Connect"
      description="Sign in with your own identity provider (Authentik, Keycloak, Authelia, Zitadel, Pocket ID…). Register Orbit as a confidential client with the callback URL as its redirect URI, and the scopes openid, email and profile."
    >
      <FieldGrid>
        <CallbackUrlField id={`${id}-callback`} url={callbackUrlOf('oidc', saved)} />
        {configured ? (
          <>
            <Field className="min-[900px]:col-span-2">
              <FieldLabel htmlFor={`${id}-issuer`}>Issuer</FieldLabel>
              <Input id={`${id}-issuer`} disabled className="font-mono" value={saved?.issuer ?? ''} />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-client-id`}>Client ID</FieldLabel>
              <Input id={`${id}-client-id`} disabled value={saved?.client_id ?? ''} />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-secret`}>Client secret</FieldLabel>
              <Input id={`${id}-secret`} disabled value={saved?.secret_set ? 'Set' : 'Not set'} />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-name`}>Display name <InfoTip text={'The sign-in button reads "Continue with" and this name.'} /></FieldLabel>
              <Input id={`${id}-name`} disabled value={displayName} />
            </Field>
          </>
        ) : null}
      </FieldGrid>
      <div className="mt-4">
        {configured ? <SetByEnvironment variable="ORBIT_OIDC_*" /> : (
          <p data-slot="oidc-off" className="text-xs text-muted-foreground">
            Off. To turn it on, set {OIDC_VARIABLES.map((variable, index) => (
              <span key={variable}>{index === 0 ? '' : index === OIDC_VARIABLES.length - 1 ? ' and ' : ', '}<code className="font-mono">{variable}</code></span>
            ))} on the server, then restart Orbit.
          </p>
        )}
      </div>
    </SettingsCard>
  )
}

function CallbackUrlField({ id, url }: { id: string; url: string }) {
  return (
    <Field className="min-[900px]:col-span-2">
      <FieldLabel htmlFor={id}>Callback URL</FieldLabel>
      <div className="flex items-center gap-2">
        <Input id={id} readOnly value={url} className="font-mono" onFocus={(event) => event.target.select()} />
        <Button type="button" variant="outline" size="icon" aria-label="Copy callback URL" onClick={() => {
          void navigator.clipboard.writeText(url).then(() => toast('Copied'), () => toast.error('Could not copy. Select and copy it manually.'))
        }}>
          <Copy />
        </Button>
      </div>
    </Field>
  )
}
