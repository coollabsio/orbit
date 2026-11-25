import { useState } from 'react'
import { View } from 'react-native'

import type { CustomPageMode, CustomPagePolicy } from '@orbit/api/generated/types.gen'

import { AdminShell, Note, OptionHint, OptionText, Segmented, SetByEnvironment, SettingsCard, SettingsRow, Toggle } from '../../../../components/settings/Shell'
import { useInstanceSettings, useSetCustomPagePolicy, useSetRegistration } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Input } from '../../../../ui'

/** The web's Admin → General: who can create an account, and which sites custom pages may show. */
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
          <CustomPagesCard policy={settings.data.custom_pages} locked={settings.data.environment.frame_origins} />
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

const MODES: readonly { value: CustomPageMode; label: string }[] = [
  { value: 'any', label: 'Any site' },
  { value: 'listed', label: 'Only these' },
  { value: 'off', label: 'Off' },
]

const MODE_HELP: Record<CustomPageMode, string> = {
  any: "Workspace owners and admins can add pages from any https site. Each user's browser allows only the sites of the pages that this user sees.",
  listed: 'Pages must be on one of these origins. A page whose origin you remove stays, but Orbit does not show it.',
  off: 'No custom pages show, and none can be added. The pages stay for later.',
}

function CustomPagesCard({ policy, locked }: { policy: CustomPagePolicy; locked: boolean }) {
  const setPolicy = useSetCustomPagePolicy()
  const saved = policy.origins.join('\n')
  const [mode, setMode] = useState(policy.mode)
  const [text, setText] = useState(saved)
  const entries = text.split(/[\s,]+/).filter(Boolean)
  const changed = mode !== policy.mode || entries.join('\n') !== saved
  const busy = locked || setPolicy.isPending
  return (
    <SettingsCard
      title="Custom pages"
      description="Workspace owners and admins can show web pages in their sidebar. Orbit frames them in a sandbox; each site must allow Orbit in its frame-ancestors."
    >
      <Segmented label="Which sites" value={mode} options={MODES} disabled={busy} onChange={setMode} />
      <Note>{MODE_HELP[mode]}</Note>
      {mode === 'listed' ? (
        <Input
          label="Allowed origins"
          help="One https origin per line, without a path."
          value={text}
          onChangeText={setText}
          editable={!busy}
          placeholder={'https://grafana.example.com\nhttps://deploys.example.com'}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
          multiline
        />
      ) : null}
      {locked ? <SetByEnvironment variable="ORBIT_FRAME_ORIGINS" /> : null}
      {setPolicy.isError ? <Note error>{errorMessage(setPolicy.error)}</Note> : null}
      {locked ? null : (
        <View className="flex-row justify-end">
          <Button
            label="Save"
            disabled={!changed || setPolicy.isPending}
            onPress={() =>
              setPolicy.mutate(
                { mode, origins: entries },
                {
                  onSuccess: (settings) => {
                    setMode(settings.custom_pages.mode)
                    setText(settings.custom_pages.origins.join('\n'))
                  },
                },
              )
            }
          />
        </View>
      )}
    </SettingsCard>
  )
}
