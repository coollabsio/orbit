import * as Clipboard from 'expo-clipboard'
import { type ReactNode, useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, Note, OptionHint, OptionText, SetByEnvironment, SettingsCard, Toggle } from '../../../../components/settings/Shell'
import type { InstanceSettingsView, SignInProviderView } from '../../../../data/admin-api'
import { PROVIDER_LABEL, type Provider } from '../../../../data/sign-in-provider'
import { useInstanceSettings, useRemoveSignInProvider, useSaveSignInProvider, useSetOidcProvision } from '../../../../hooks/admin'
import { useOrbit } from '../../../../orbit-context'
import { errorMessage } from '../../../../session-context'
import { Button, Input, Text } from '../../../../ui'

const PROVIDERS: { provider: Provider; title: string; variable: string; description: string }[] = [
  { provider: 'google', title: 'Google', variable: 'ORBIT_GOOGLE_*', description: 'Create a client of type "Web application" in Google Cloud Console (APIs & Services → Credentials) and add the callback URL as an authorized redirect URI.' },
  { provider: 'github', title: 'GitHub', variable: 'ORBIT_GITHUB_*', description: 'Register an app in GitHub (Settings → Developer settings → OAuth Apps) with the callback URL as its authorization callback URL.' },
  { provider: 'oidc', title: 'OpenID Connect', variable: 'ORBIT_OIDC_*', description: 'Sign in with your own identity provider (Authentik, Keycloak, Authelia, Zitadel, Pocket ID…). Register Orbit as a confidential client with the callback URL as its redirect URI, and the scopes openid, email and profile.' },
]

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
          {PROVIDERS.map(({ provider, title, variable, description }) => {
            const saved = settings.data.sign_in_providers.find((item) => item.provider === provider) ?? null
            const lockedBy = settings.data.environment.sign_in_providers.includes(provider) ? variable : null
            return (
              <ProviderCard key={provider} provider={provider} title={title} description={description} saved={saved} lockedBy={lockedBy}>
                {provider === 'oidc' && saved ? <ProvisionSwitch on={settings.data.oidc_provision} locked={settings.data.environment.oidc_provision} /> : null}
              </ProviderCard>
            )
          })}
        </>
      ) : null}
    </AdminShell>
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
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const oidc = provider === 'oidc'
  const name = oidc ? title : PROVIDER_LABEL[provider]
  const ready = clientId.trim() !== '' && (saved?.secret_set || secret !== '') && (!oidc || issuer.trim() !== '')
  // after a save or a removal the form shows what the server stored
  const stored = {
    onSuccess: (settings: InstanceSettingsView) => {
      const stored = settings.sign_in_providers.find((item) => item.provider === provider)
      setClientId(stored?.client_id ?? '')
      setIssuer(stored?.issuer ?? '')
      setDisplayName(stored?.display_name ?? '')
      setSecret('')
    },
  }

  return (
    <SettingsCard title={title} description={description}>
      <View className="gap-4">
        <CallbackUrlField url={useCallbackUrl(provider, saved)} />
        {oidc ? (
          <Input
            label="Issuer"
            required
            help="The issuer URL of the provider, for example https://auth.example.com/application/o/orbit/. Orbit reads {issuer}/.well-known/openid-configuration. If you change it later, people connect their accounts again."
            keyboardType="url"
            editable={!locked}
            value={issuer}
            onChangeText={setIssuer}
            autoCapitalize="none"
            autoCorrect={false}
            autoComplete="off"
            spellCheck={false}
          />
        ) : null}
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
        {oidc ? <Input label="Display name" help={'The sign-in button reads "Continue with" and this name. Empty: SSO.'} placeholder="SSO" maxLength={50} editable={!locked} value={displayName} onChangeText={setDisplayName} /> : null}
        {children}
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
              const body = { client_id: clientId.trim(), client_secret: secret || null }
              save.mutate({ provider, body: oidc ? { ...body, issuer: issuer.trim(), display_name: displayName.trim() || null } : body }, stored)
            }}
          />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

/** Whether a first OpenID Connect sign-in creates an account, also when open registration is off. */
function ProvisionSwitch({ on, locked }: { on: boolean; locked: boolean }) {
  const setProvision = useSetOidcProvision()
  return (
    <View className="flex-row items-center gap-3">
      <OptionText title="Create accounts at first sign-in" help="On: anyone who can sign in at your identity provider gets an Orbit account at their first sign-in, also when open registration is off. Email sign-up does not change. Off: a first sign-in creates an account only when registration is open.">
        {locked ? <SetByEnvironment variable="ORBIT_OIDC_PROVISION" /> : null}
        {setProvision.isError ? <OptionHint error>{errorMessage(setProvision.error)}</OptionHint> : null}
      </OptionText>
      <Toggle label="Create accounts at first sign-in" value={on} disabled={locked || setProvision.isPending} onChange={(checked) => setProvision.mutate(checked)} />
    </View>
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
