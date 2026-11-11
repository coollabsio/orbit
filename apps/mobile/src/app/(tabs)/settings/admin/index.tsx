import { View } from 'react-native'

import { AdminShell, Note, OptionHint, OptionText, SetByEnvironment, SettingsCard, SettingsRow, Toggle } from '../../../../components/settings/Shell'
import { useInstanceSettings, useSetRegistration } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button } from '../../../../ui'

/** The web's Admin → General: who can create an account. */
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
        <RegistrationCard open={settings.data.registration_open} emailReady={settings.data.smtp != null} locked={settings.data.environment.registration_open} />
      ) : null}
    </AdminShell>
  )
}

function RegistrationCard({ open, emailReady, locked }: { open: boolean; emailReady: boolean; locked: boolean }) {
  const setRegistration = useSetRegistration()
  return (
    <SettingsCard title="Registration" description="Who can create an account on this Orbit." flush>
      <SettingsRow first>
        <OptionText title="Open registration" help={emailReady ? 'On: anyone can create an account with a link sent to their email. Off: people join only by invitation.' : undefined}>
          {emailReady ? null : <OptionHint>Set up a mail server in Email first: the sign-up link goes out by email.</OptionHint>}
          {locked ? <SetByEnvironment variable="ORBIT_OPEN_REGISTRATION" /> : null}
          {setRegistration.isError ? <OptionHint error>{errorMessage(setRegistration.error)}</OptionHint> : null}
        </OptionText>
        <Toggle label="Open registration" value={open} disabled={locked || (!emailReady && !open) || setRegistration.isPending} onChange={(checked) => setRegistration.mutate(checked)} />
      </SettingsRow>
    </SettingsCard>
  )
}
