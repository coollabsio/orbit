import { type ReactNode, useId, useState } from 'react'
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
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useRemoveSignInProvider, useSaveSignInProvider, useSetOidcProvision } from '@/features/admin/api'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'
import { PROVIDER_NAMES } from '@/features/auth/providerSignIn'
import { FieldGrid, RequiredMark } from '@/features/settings/components/SettingsParts'
import { LoadingText } from '@/components/ui/spinner'
import { writeClipboard } from '@/lib/clipboard'

const PROVIDERS: { provider: Provider; title: string; variable: string; description: string }[] = [
  { provider: 'google', title: 'Google', variable: 'ORBIT_GOOGLE_*', description: 'Create a client of type "Web application" in Google Cloud Console (APIs & Services → Credentials) and add the callback URL as an authorized redirect URI.' },
  { provider: 'github', title: 'GitHub', variable: 'ORBIT_GITHUB_*', description: 'Register an app in GitHub (Settings → Developer settings → OAuth Apps) with the callback URL as its authorization callback URL.' },
  { provider: 'oidc', title: 'OpenID Connect', variable: 'ORBIT_OIDC_*', description: 'Sign in with your own identity provider (Authentik, Keycloak, Authelia, Zitadel, Pocket ID…). Register Orbit as a confidential client with the callback URL as its redirect URI, and the scopes openid, email and profile.' },
]

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
      {PROVIDERS.map(({ provider, title, variable, description }) => {
        const saved = settings.data.sign_in_providers.find((item) => item.provider === provider) ?? null
        const lockedBy = settings.data.environment.sign_in_providers.includes(provider) ? variable : null
        // Remount on save so the form shows what the server stored.
        return (
          <ProviderCard key={`${provider}:${JSON.stringify(saved)}`} provider={provider} title={title} description={description} saved={saved} lockedBy={lockedBy}>
            {provider === 'oidc' && saved ? <ProvisionSwitch on={settings.data.oidc_provision} locked={settings.data.environment.oidc_provision} /> : null}
          </ProviderCard>
        )
      })}
    </>
  )
}

/** `lockedBy`: the environment variables that set this provider. OpenID Connect also has an issuer and a button name. */
function ProviderCard({ provider, title, description, saved, lockedBy, children }: { provider: Provider; title: string; description: string; saved: SignInProviderView | null; lockedBy: string | null; children?: ReactNode }) {
  const locked = lockedBy != null
  const save = useSaveSignInProvider()
  const remove = useRemoveSignInProvider()
  const [clientId, setClientId] = useState(saved?.client_id ?? '')
  const [secret, setSecret] = useState('')
  const [issuer, setIssuer] = useState(saved?.issuer ?? '')
  const [displayName, setDisplayName] = useState(saved?.display_name ?? '')
  const id = useId()
  const oidc = provider === 'oidc'
  const name = oidc ? title : PROVIDER_NAMES[provider]

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
      const body = { client_id: clientId.trim(), client_secret: secret || null }
      save.mutate({ provider, body: oidc ? { ...body, issuer: issuer.trim(), display_name: displayName.trim() || null } : body }, {
        onSuccess: () => toast(`${name} saved`),
        onError: (error) => toast.error(problemText(error, `${name} could not be saved.`)),
      })
    }}>
      <SettingsCard title={title} description={description}>
        <FieldGrid>
          <CallbackUrlField id={`${id}-callback`} url={callbackUrlOf(provider, saved)} />
          {oidc ? (
            <Field className="min-[900px]:col-span-2">
              <FieldLabel htmlFor={`${id}-issuer`}>Issuer <RequiredMark /> <InfoTip text="The issuer URL of the provider, for example https://auth.example.com/application/o/orbit/. Orbit reads {issuer}/.well-known/openid-configuration. If you change it later, people connect their accounts again." /></FieldLabel>
              <Input id={`${id}-issuer`} type="url" required autoComplete="off" spellCheck={false} className="font-mono" disabled={locked} value={issuer} onChange={(event) => setIssuer(event.target.value)} />
            </Field>
          ) : null}
          <Field>
            <FieldLabel htmlFor={`${id}-client-id`}>Client ID <RequiredMark /></FieldLabel>
            <Input id={`${id}-client-id`} required autoComplete="off" spellCheck={false} disabled={locked} value={clientId} onChange={(event) => setClientId(event.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor={`${id}-secret`}>Client secret {saved?.secret_set ? null : <RequiredMark />} <InfoTip text="Stored encrypted." /></FieldLabel>
            <PasswordInput id={`${id}-secret`} autoComplete="new-password" required={!saved?.secret_set} placeholder={saved?.secret_set ? 'Saved. Leave empty to keep it.' : ''} disabled={locked} value={secret} onChange={(event) => setSecret(event.target.value)} />
          </Field>
          {oidc ? (
            <Field>
              <FieldLabel htmlFor={`${id}-name`}>Display name <InfoTip text={'The sign-in button reads "Continue with" and this name. Empty: SSO.'} /></FieldLabel>
              <Input id={`${id}-name`} maxLength={50} placeholder="SSO" disabled={locked} value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
            </Field>
          ) : null}
          {children}
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

/** Whether a first OpenID Connect sign-in creates an account, also when open registration is off. */
function ProvisionSwitch({ on, locked }: { on: boolean; locked: boolean }) {
  const setProvision = useSetOidcProvision()
  const id = useId()
  return (
    <Field className="min-[900px]:col-span-2">
      <div className="flex items-center justify-between gap-3">
        <FieldLabel htmlFor={id}>Create accounts at first sign-in <InfoTip text="On: anyone who can sign in at your identity provider gets an Orbit account at their first sign-in, also when open registration is off. Email sign-up does not change. Off: a first sign-in creates an account only when registration is open." /></FieldLabel>
        <Switch
          id={id}
          checked={on}
          disabled={locked || setProvision.isPending}
          onCheckedChange={(checked: boolean) => setProvision.mutate(checked, {
            onSuccess: () => toast(checked ? 'OpenID Connect creates accounts' : 'OpenID Connect no longer creates accounts'),
            onError: (error) => toast.error(problemText(error, 'The setting could not be changed.')),
          })}
        />
      </div>
      {locked ? <SetByEnvironment variable="ORBIT_OIDC_PROVISION" /> : null}
    </Field>
  )
}

function CallbackUrlField({ id, url }: { id: string; url: string }) {
  return (
    <Field className="min-[900px]:col-span-2">
      <FieldLabel htmlFor={id}>Callback URL</FieldLabel>
      <div className="flex items-center gap-2">
        <Input id={id} readOnly value={url} className="font-mono" onFocus={(event) => event.target.select()} />
        <Button type="button" variant="outline" size="icon" aria-label="Copy callback URL" onClick={() => {
          void writeClipboard(url).then(() => toast('Copied'), () => toast.error('Could not copy. Select and copy it manually.'))
        }}>
          <Copy />
        </Button>
      </div>
    </Field>
  )
}
