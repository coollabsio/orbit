import * as Clipboard from 'expo-clipboard'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, Note, SetByEnvironment, SettingsCard } from '../../../../components/settings/Shell'
import type { InstanceSettingsView, SignInProviderView } from '../../../../data/admin-api'
import { PROVIDER_LABEL, type Provider } from '../../../../data/sign-in-provider'
import { useAuthOptions } from '../../../../hooks/account'
import { useInstanceSettings, useRemoveSignInProvider, useSaveSignInProvider } from '../../../../hooks/admin'
import { useOrbit } from '../../../../orbit-context'
import { errorMessage } from '../../../../session-context'
import { Button, Input, Text } from '../../../../ui'

/** The providers an admin can save here; OpenID Connect is set by environment variables only. */
type SavableProvider = Exclude<Provider, 'oidc'>

const PROVIDERS: { provider: SavableProvider; variable: string; description: string }[] = [
  { provider: 'google', variable: 'ORBIT_GOOGLE_*', description: 'Create a client of type "Web application" in Google Cloud Console (APIs & Services → Credentials) and add the callback URL as an authorized redirect URI.' },
  { provider: 'github', variable: 'ORBIT_GITHUB_*', description: 'Register an app in GitHub (Settings → Developer settings → OAuth Apps) with the callback URL as its authorization callback URL.' },
]

const OIDC_VARIABLES = ['ORBIT_OIDC_ISSUER', 'ORBIT_OIDC_CLIENT_ID', 'ORBIT_OIDC_CLIENT_SECRET', 'ORBIT_OIDC_DISPLAY_NAME']

/** The server reports the callback URL of saved providers only; before that it is the server's own. */
function useCallbackUrl(provider: Provider, saved: SignInProviderView | null) {
  const { session } = useOrbit()
  return saved?.callback_url ?? `${session.server.replace(/\/+$/, '')}/api/v1/auth/providers/${provider}/callback`
}

/** The web's Admin → Sign-in providers: the client that Google and GitHub issued to this Orbit, and the OpenID Connect values. */
export default function AdminSignInProvidersScreen() {
  const settings = useInstanceSettings()
  return (
    <AdminShell onRefresh={() => settings.refetch()}>
      {settings.isPending ? <SettingsCard title="Sign-in providers"><Note loading>Loading settings…</Note></SettingsCard> : null}
      {settings.isError ? (
        <SettingsCard title="Sign-in providers">
          <View className="flex-row items-center gap-2">
            <Note error>Settings could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void settings.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {settings.data ? (
        <>
          {PROVIDERS.map(({ provider, variable, description }) => {
            const saved = settings.data.sign_in_providers.find((item) => item.provider === provider) ?? null
            const lockedBy = settings.data.environment.sign_in_providers.includes(provider) ? variable : null
            return <ProviderCard key={provider} provider={provider} description={description} saved={saved} lockedBy={lockedBy} />
          })}
          <OidcCard configured={settings.data.environment.sign_in_providers.includes('oidc')} saved={settings.data.sign_in_providers.find((item) => item.provider === 'oidc') ?? null} />
        </>
      ) : null}
    </AdminShell>
  )
}

/** `lockedBy`: the environment variables that set this provider. */
function ProviderCard({ provider, description, saved, lockedBy }: { provider: SavableProvider; description: string; saved: SignInProviderView | null; lockedBy: string | null }) {
  const locked = lockedBy != null
  const save = useSaveSignInProvider()
  const remove = useRemoveSignInProvider()
  const [clientId, setClientId] = useState(saved?.client_id ?? '')
  const [secret, setSecret] = useState('')
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const name = PROVIDER_LABEL[provider]
  const ready = clientId.trim() !== '' && (saved?.secret_set || secret !== '')
  // after a save or a removal the form shows what the server stored
  const stored = {
    onSuccess: (settings: InstanceSettingsView) => {
      setClientId(settings.sign_in_providers.find((item) => item.provider === provider)?.client_id ?? '')
      setSecret('')
    },
  }

  return (
    <SettingsCard title={name} description={description}>
      <View className="gap-4">
        <CallbackUrlField url={useCallbackUrl(provider, saved)} />
        <Input label="Client ID" required editable={!locked} value={clientId} onChangeText={setClientId} autoCapitalize="none" autoCorrect={false} autoComplete="off" spellCheck={false} />
        <Input
          label="Client secret"
          required={!saved?.secret_set}
          help="Stored encrypted."
          secureTextEntry
          autoComplete="new-password"
          placeholder={saved?.secret_set ? 'Saved. Leave empty to keep it.' : undefined}
          editable={!locked}
          value={secret}
          onChangeText={setSecret}
        />
        {lockedBy ? <SetByEnvironment variable={lockedBy} /> : null}
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {remove.isError ? <Note error>{errorMessage(remove.error)}</Note> : null}
        {save.isSuccess ? <Note>{name} saved.</Note> : null}
        {remove.isSuccess ? <Note>{name} removed.</Note> : null}
        <View className="flex-row items-center justify-end gap-2">
          {saved ? (
            <Button
              label="Remove"
              variant="destructive"
              disabled={locked || remove.isPending}
              onPress={() =>
                setConfirm({
                  title: `Remove ${name}?`,
                  description: `"Continue with ${name}" disappears from the sign-in page. Accounts that only sign in with ${name} need a password reset to get back in.`,
                  confirmLabel: 'Remove',
                  danger: true,
                  onConfirm: () => {
                    save.reset()
                    remove.mutate(provider, stored)
                  },
                })
              }
            />
          ) : null}
          <Button label={save.isPending ? 'Saving…' : `Save ${name}`} disabled={locked || save.isPending || !ready} onPress={() => {
              remove.reset()
              save.mutate({ provider, body: { client_id: clientId.trim(), client_secret: secret || null } }, stored)
            }}
          />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

/** A generic OpenID Connect provider (Authentik, Keycloak, …). Only `ORBIT_OIDC_*` sets it, so the card shows the values without letting Admin change them. */
function OidcCard({ configured, saved }: { configured: boolean; saved: SignInProviderView | null }) {
  const displayName = useAuthOptions().data?.oidc_name ?? ''
  return (
    <SettingsCard
      title="OpenID Connect"
      description="Sign in with your own identity provider (Authentik, Keycloak, Authelia, Zitadel, Pocket ID…). Register Orbit as a confidential client with the callback URL as its redirect URI, and the scopes openid, email and profile."
    >
      <View className="gap-4">
        <CallbackUrlField url={useCallbackUrl('oidc', saved)} />
        {configured ? (
          <>
            <Input label="Issuer" editable={false} value={saved?.issuer ?? ''} />
            <Input label="Client ID" editable={false} value={saved?.client_id ?? ''} />
            <Input label="Client secret" editable={false} value={saved?.secret_set ? 'Set' : 'Not set'} />
            <Input label="Display name" help={'The sign-in button reads "Continue with" and this name.'} editable={false} value={displayName} />
            <SetByEnvironment variable="ORBIT_OIDC_*" />
          </>
        ) : (
          <Text className="text-[12px] leading-[17px] text-muted-foreground">
            Off. To turn it on, set{' '}
            {OIDC_VARIABLES.map((variable, index) => (
              <Text key={variable} className="text-[12px] leading-[17px] text-muted-foreground">
                {index === 0 ? '' : index === OIDC_VARIABLES.length - 1 ? ' and ' : ', '}
                <Text className="font-mono text-[12px] leading-[17px] text-muted-foreground">{variable}</Text>
              </Text>
            ))}{' '}
            on the server, then restart Orbit.
          </Text>
        )}
      </View>
    </SettingsCard>
  )
}

function CallbackUrlField({ url }: { url: string }) {
  const [copied, setCopied] = useState<'copied' | 'failed'>()
  return (
    <Input
      label="Callback URL"
      editable={false}
      selectTextOnFocus
      value={url}
      error={copied === 'failed' ? 'Could not copy. Select and copy it manually.' : undefined}
      accessory={
        <Button
          label={copied === 'copied' ? 'Copied' : 'Copy'}
          icon="copy"
          variant="outline"
          onPress={() => void Clipboard.setStringAsync(url).then((done) => setCopied(done ? 'copied' : 'failed'), () => setCopied('failed'))}
        />
      }
    />
  )
}
