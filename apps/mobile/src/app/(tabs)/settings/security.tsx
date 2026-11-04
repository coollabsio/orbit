import { useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Image, Linking, Share, View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest, Dialog, PromptDialog, type PromptRequest } from '../../../components/settings/Dialog'
import { ACCOUNT_NAV, Badge, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell } from '../../../components/settings/Shell'
import { accountKeys, type TotpSetup, twoFactorSummary } from '../../../data/account-api'
import { type Provider, PROVIDER_LABEL, type ProviderOutcome, providerErrorMessage, providerRows, startProviderSignIn, withReauthentication } from '../../../data/sign-in-provider'
import { useAuthOptions, useConfirmIdentity, useConnectedAccounts, useDisconnectAccount, useTwoFactor, useTwoFactorWrites } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { providerDevice } from '../../../sign-in-provider'
import { Button, Input, Text } from '../../../ui'

/**
 * The web's Security page: the authenticator app (set up, new recovery codes, turn off) and the connected Google and
 * GitHub accounts. An account with a password confirms each authenticator step with it; one without confirms with a
 * fresh provider sign-in when the server asks. Passkeys need the browser's WebAuthn prompt, which Expo Go does not
 * offer, so they stay on the web.
 */
export default function SecurityScreen() {
  const { client, user } = useOrbit()
  const queryClient = useQueryClient()
  const status = useTwoFactor()
  const writes = useTwoFactorWrites()
  const accounts = useConnectedAccounts()
  const options = useAuthOptions()
  const disconnect = useDisconnectAccount()
  const [prompt, setPrompt] = useState<PromptRequest | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [setup, setSetup] = useState<TotpSetup | null>(null)
  const [codes, setCodes] = useState<{ title: string; codes: string[] } | null>(null)
  const [twoFactorError, setTwoFactorError] = useState<string | null>(null)
  const [accountsError, setAccountsError] = useState<string | null>(null)
  const [pending, setPending] = useState<Provider | 'two_factor' | null>(null)
  const enabled = status.data?.totp_enabled === true
  const rows = providerRows(options.data?.providers ?? [], accounts.data ?? [])

  const confirmIdentity = useConfirmIdentity()

  const confirmed = (title: string, description: string, confirmLabel: string, then: (password?: string) => Promise<unknown>) => {
    if (user.has_password) return setPrompt({ title, description, label: 'Current password', confirmLabel, secure: true, onSubmit: then })
    setConfirm({
      title,
      description,
      confirmLabel,
      onConfirm: () => {
        setTwoFactorError(null)
        setPending('two_factor')
        withReauthentication(() => then(), confirmIdentity)
          .catch((error: unknown) => setTwoFactorError(providerErrorMessage(error)))
          .finally(() => setPending(null))
      },
    })
  }

  const connected = (outcome: ProviderOutcome | null) => {
    if (outcome?.kind === 'connected') void queryClient.invalidateQueries({ queryKey: accountKeys.connectedAccounts })
  }

  /** A connected account is a new way into the account: one with a password confirms it first. */
  const connect = (provider: Provider) => {
    setAccountsError(null)
    const label = PROVIDER_LABEL[provider]
    if (user.has_password) {
      return setPrompt({
        title: `Connect ${label}`,
        description: `Confirm your password, then sign in with ${label}.`,
        label: 'Current password',
        confirmLabel: 'Continue',
        secure: true,
        onSubmit: (password) =>
          startProviderSignIn(client, provider, 'connect', providerDevice, password).then(connected, (error: unknown) => {
            throw new Error(providerErrorMessage(error))
          }),
      })
    }
    setPending(provider)
    withReauthentication(() => startProviderSignIn(client, provider, 'connect', providerDevice), confirmIdentity)
      .then(connected)
      .catch((error: unknown) => setAccountsError(providerErrorMessage(error)))
      .finally(() => setPending(null))
  }

  const askDisconnect = (provider: Provider) =>
    setConfirm({
      title: `Disconnect ${PROVIDER_LABEL[provider]}?`,
      description: `You can no longer sign in to Orbit with ${PROVIDER_LABEL[provider]}.`,
      confirmLabel: 'Disconnect',
      danger: true,
      onConfirm: () => {
        setAccountsError(null)
        disconnect.mutate(provider, { onError: (error) => setAccountsError(providerErrorMessage(error)) })
      },
    })

  return (
    <SettingsShell
      title="Account settings"
      nav={ACCOUNT_NAV}
      refreshing={status.isRefetching || accounts.isRefetching}
      onRefresh={() => {
        void status.refetch()
        void accounts.refetch()
      }}
    >
      <SettingsCard
        title="Authenticator app"
        description="Sign-in asks for a code from an app such as 1Password, Authy or Google Authenticator after your password."
        actions={
          status.data ? (
            enabled ? (
              <>
                <Button
                  label="New recovery codes"
                  variant="outline"
                  disabled={pending === 'two_factor'}
                  onPress={() =>
                    confirmed('New recovery codes', 'Your current recovery codes stop working.', 'Make new codes', async (password) =>
                      setCodes({ title: 'New recovery codes', codes: await writes.regenerate.mutateAsync(password) }),
                    )
                  }
                />
                <Button
                  label="Turn off"
                  variant="destructive"
                  disabled={pending === 'two_factor'}
                  onPress={() =>
                    confirmed('Turn off two-factor sign-in?', 'Sign-in asks only for your password again, and your recovery codes stop working.', 'Turn off', (password) =>
                      writes.disable.mutateAsync(password),
                    )
                  }
                />
              </>
            ) : (
              <Button
                label="Set up"
                variant="outline"
                loading={pending === 'two_factor'}
                onPress={() =>
                  confirmed('Set up an authenticator app', user.has_password ? 'Confirm your password to start.' : 'Orbit may ask you to sign in again to start.', 'Continue', async (password) =>
                    setSetup(await writes.setup.mutateAsync(password)),
                  )
                }
              />
            )
          ) : undefined
        }
        flush
      >
        {status.isPending ? <View className="p-4"><Note>Loading…</Note></View> : null}
        {status.isError ? <View className="p-4"><Note error>Two-factor sign-in could not be loaded.</Note></View> : null}
        {status.data ? (
          <SettingsRow first>
            <RowIcon icon="mobile" />
            <RowText title="Two-factor sign-in" subtitle={twoFactorSummary(status.data)} badges={<Badge label={enabled ? 'On' : 'Off'} tone={enabled ? 'primary' : 'secondary'} />} />
          </SettingsRow>
        ) : null}
        {twoFactorError ? <View className="px-4 pb-4"><Note error>{twoFactorError}</Note></View> : null}
      </SettingsCard>

      {rows.length > 0 ? (
        <SettingsCard title="Connected accounts" description="Sign in to Orbit with these accounts instead of a password." flush>
          {rows.map(({ provider, account }, index) => (
            <SettingsRow key={provider} first={index === 0}>
              <RowIcon icon={provider} />
              <RowText title={PROVIDER_LABEL[provider]} subtitle={account ? (account.email ?? 'Connected') : 'Not connected'} />
              {account ? (
                <Button label="Disconnect" variant="outline" size="sm" disabled={disconnect.isPending} onPress={() => askDisconnect(provider)} />
              ) : (
                <Button label="Connect" variant="outline" size="sm" loading={pending === provider} disabled={pending !== null} onPress={() => connect(provider)} />
              )}
            </SettingsRow>
          ))}
          {accounts.isError ? <View className="px-4 pb-4"><Note error>Connected accounts could not be loaded.</Note></View> : null}
          {accountsError ? <View className="px-4 pb-4"><Note error>{accountsError}</Note></View> : null}
        </SettingsCard>
      ) : null}

      <SettingsCard title="Passkeys" description="Sign in with your fingerprint, face or device screen lock instead of a password.">
        <Note>Add and remove passkeys in Orbit in a browser: creating one needs the browser's passkey prompt.</Note>
      </SettingsCard>

      <PromptDialog request={prompt} onClose={() => setPrompt(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      <SetupDialog
        setup={setup}
        onEnabled={(list) => {
          setSetup(null)
          setCodes({ title: 'Two-factor sign-in is on', codes: list })
        }}
        onClose={() => setSetup(null)}
      />
      <RecoveryCodesDialog codes={codes} onClose={() => setCodes(null)} />
    </SettingsShell>
  )
}

/** Step two of the setup: the QR code (for another device), the key, the authenticator app link, then the code. */
function SetupDialog({ setup, onEnabled, onClose }: { setup: TotpSetup | null; onEnabled: (codes: string[]) => void; onClose: () => void }) {
  const { enable } = useTwoFactorWrites()
  const [code, setCode] = useState('')
  const submit = () => {
    if (!code.trim()) return
    enable.mutate(code.trim(), {
      onSuccess: (codes) => {
        setCode('')
        onEnabled(codes)
      },
    })
  }
  return (
    <Dialog
      visible={setup !== null}
      title="Add Orbit to your authenticator app"
      description="Open the setup link in an authenticator app on this phone, or scan the QR code from another device, or enter the key by hand. Then enter the code the app shows."
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={enable.isPending ? 'Turning on…' : 'Turn on'} disabled={enable.isPending || !code.trim()} onPress={submit} />
        </>
      }
    >
      {setup ? (
        <>
          <Image accessibilityLabel="QR code for your authenticator app" source={{ uri: setup.qr_code }} className="size-42 self-center rounded-md bg-white" />
          <Button label="Open in authenticator app" icon="key" variant="outline" onPress={() => void Linking.openURL(setup.otpauth_url).catch(() => undefined)} />
          <CopyValue label="Setup key" value={setup.secret} />
          <Input label="Code from the app" value={code} onChangeText={setCode} keyboardType="number-pad" autoComplete="one-time-code" textContentType="oneTimeCode" returnKeyType="done" onSubmitEditing={submit} error={enable.isError ? errorMessage(enable.error) : undefined} />
        </>
      ) : null}
    </Dialog>
  )
}

function RecoveryCodesDialog({ codes, onClose }: { codes: { title: string; codes: string[] } | null; onClose: () => void }) {
  const text = codes ? `Orbit recovery codes\nEach code signs in once in place of an authenticator app code.\n\n${codes.codes.join('\n')}\n` : ''
  return (
    <Dialog
      visible={codes !== null}
      title={codes?.title ?? ''}
      description="Save these recovery codes somewhere safe. Each one signs you in once if you lose your authenticator app. They are shown only now."
      onClose={onClose}
      footer={
        <>
          <Button label="Share" icon="send" variant="outline" onPress={() => void Share.share({ message: text })} />
          <Button label="Done" onPress={onClose} />
        </>
      }
    >
      <View accessibilityLabel="Recovery codes" className="flex-row flex-wrap gap-y-1.5 p-4 rounded-md bg-muted">
        {codes?.codes.map((code) => (
          <Text key={code} selectable className="w-[50%] font-mono text-[13px] text-foreground leading-[21px]">
            {code}
          </Text>
        ))}
      </View>
    </Dialog>
  )
}

/** A read-only value to select or share (the app has no clipboard module). */
function CopyValue({ label, value }: { label: string; value: string }) {
  return (
    <View className="gap-2">
      <Text className="font-noto-medium text-[14px]">{label}</Text>
      <View className="flex-row items-center gap-2">
        <Text selectable className="flex-1 font-mono text-[13px] text-foreground p-2 rounded-md border border-input leading-[21px]">
          {value}
        </Text>
        <Button label="Share" variant="outline" size="sm" onPress={() => void Share.share({ message: value })} />
      </View>
    </View>
  )
}
