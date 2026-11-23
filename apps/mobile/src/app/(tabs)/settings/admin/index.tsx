import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, Note, OptionHint, OptionText, SetByEnvironment, SettingsCard, SettingsRow, Toggle } from '../../../../components/settings/Shell'
import { useInstanceSettings, useRemoveKlipy, useSaveKlipy, useSetRegistration } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Input } from '../../../../ui'

/** The web's Admin → General: who can create an account, and the KLIPY key for GIFs and stickers in chat. */
export default function AdminGeneralScreen() {
  const settings = useInstanceSettings()
  return (
    <AdminShell onRefresh={() => settings.refetch()}>
      {settings.isPending ? <SettingsCard title="General"><Note loading>Loading settings…</Note></SettingsCard> : null}
      {settings.isError ? (
        <SettingsCard title="General">
          <View className="flex-row items-center gap-2">
            <Note error>Settings could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void settings.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {settings.data ? (
        <>
          <RegistrationCard open={settings.data.registration_open} ready={settings.data.registration_ready} locked={settings.data.environment.registration_open} />
          <KlipyCard keySet={settings.data.klipy.key_set} locked={settings.data.environment.klipy} />
        </>
      ) : null}
    </AdminShell>
  )
}

function RegistrationCard({ open, ready, locked }: { open: boolean; ready: boolean; locked: boolean }) {
  const setRegistration = useSetRegistration()
  return (
    <SettingsCard title="Registration" description="Who can create an account on this Orbit." flush>
      <SettingsRow first>
        <OptionText title="Open registration" help={ready ? 'On: anyone can create an account, with a link sent to their email or with a sign-in provider. Off: people join only by invitation.' : undefined}>
          {ready ? null : <OptionHint>Set up a mail server in Email or a sign-in provider first: it confirms the email of a new account.</OptionHint>}
          {locked ? <SetByEnvironment variable="ORBIT_OPEN_REGISTRATION" /> : null}
          {setRegistration.isError ? <OptionHint error>{errorMessage(setRegistration.error)}</OptionHint> : null}
        </OptionText>
        <Toggle label="Open registration" value={open} disabled={locked || (!ready && !open) || setRegistration.isPending} onChange={(checked) => setRegistration.mutate(checked)} />
      </SettingsRow>
    </SettingsCard>
  )
}

function KlipyCard({ keySet, locked }: { keySet: boolean; locked: boolean }) {
  const save = useSaveKlipy()
  const remove = useRemoveKlipy()
  const [apiKey, setApiKey] = useState('')
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const edit = (value: string) => {
    save.reset()
    setApiKey(value)
  }

  return (
    <SettingsCard
      title="GIFs and stickers"
      description="With a KLIPY API key, chat members can search and send GIFs and stickers from KLIPY. Orbit searches KLIPY for them, so the app never sees the key."
      actions={
        keySet && !locked ? (
          <Button
            label="Remove"
            variant="destructive"
            disabled={remove.isPending}
            onPress={() =>
              setConfirm({
                title: 'Remove the KLIPY key?',
                description: 'Chat members can no longer send GIFs and stickers from KLIPY. Messages that have one keep it.',
                confirmLabel: 'Remove',
                danger: true,
                onConfirm: () => remove.mutate(undefined, { onSuccess: () => setApiKey('') }),
              })
            }
          />
        ) : undefined
      }
    >
      <View className="gap-4">
        <Input
          label="KLIPY API key"
          help="The key of your app at KLIPY (klipy.com). Orbit stores it encrypted and never shows it again."
          secureTextEntry
          autoCapitalize="none"
          autoCorrect={false}
          autoComplete="off"
          placeholder={keySet ? 'Saved. Enter a new key to replace it.' : undefined}
          editable={!locked}
          value={apiKey}
          onChangeText={edit}
        />
        {locked ? <SetByEnvironment variable="ORBIT_KLIPY_API_KEY" /> : null}
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {remove.isError ? <Note error>{errorMessage(remove.error)}</Note> : null}
        {save.isSuccess ? <Note>KLIPY key saved.</Note> : null}
        <View className="flex-row justify-end">
          <Button
            label={save.isPending ? 'Saving…' : 'Save key'}
            disabled={locked || save.isPending || apiKey.trim() === ''}
            onPress={() => save.mutate(apiKey, { onSuccess: () => setApiKey('') })}
          />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}
