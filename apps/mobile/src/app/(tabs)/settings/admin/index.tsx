import { useState } from 'react'
import { View } from 'react-native'

import { AdminShell, Note, OptionHint, OptionText, SetByEnvironment, SettingsCard, SettingsRow, Toggle } from '../../../../components/settings/Shell'
import { useInstanceSettings, useSetFrameOrigins, useSetRegistration } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Input } from '../../../../ui'

/** The web's Admin → General: who can create an account, and the custom page origins. */
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
          <FrameOriginsCard origins={settings.data.frame_origins} locked={settings.data.environment.frame_origins} />
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

function FrameOriginsCard({ origins, locked }: { origins: string[]; locked: boolean }) {
  const setFrameOrigins = useSetFrameOrigins()
  const saved = origins.join('\n')
  const [text, setText] = useState(saved)
  const entries = text.split(/[\s,]+/).filter(Boolean)
  const changed = entries.join('\n') !== saved
  return (
    <SettingsCard
      title="Custom page origins"
      description="Workspace owners and admins can show pages from these sites in their sidebar. Orbit frames them in a sandbox; each site must allow Orbit in its frame-ancestors."
    >
      <Input
        label="Custom page origins"
        help="One https origin per line, without a path. At most 20."
        value={text}
        onChangeText={setText}
        editable={!locked && !setFrameOrigins.isPending}
        placeholder={'https://grafana.example.com\nhttps://deploys.example.com'}
        autoCapitalize="none"
        autoCorrect={false}
        keyboardType="url"
        multiline
      />
      {locked ? <SetByEnvironment variable="ORBIT_FRAME_ORIGINS" /> : null}
      {setFrameOrigins.isError ? <Note error>{errorMessage(setFrameOrigins.error)}</Note> : null}
      {locked ? null : (
        <View className="flex-row justify-end">
          <Button
            label="Save"
            disabled={!changed || setFrameOrigins.isPending}
            onPress={() => setFrameOrigins.mutate(entries, { onSuccess: (settings) => setText(settings.frame_origins.join('\n')) })}
          />
        </View>
      )}
    </SettingsCard>
  )
}
