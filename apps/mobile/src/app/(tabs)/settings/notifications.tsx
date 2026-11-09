import { View } from 'react-native'

import { ACCOUNT_NAV, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell, Toggle } from '../../../components/settings/Shell'
import { NOTIFICATION_CHOICES } from '../../../data/account-api'
import { shortDate } from '../../../data/format'
import { useNotificationPrefs, usePushDevices, useRemovePushDevice, useSaveNotificationPrefs } from '../../../hooks/account'
import { errorMessage } from '../../../session-context'
import { Button, Text } from '../../../ui'

/**
 * The web's Notifications page. This device says what the app can honestly say: it gets no push notifications yet.
 * Devices lists the browsers that do, from the server. Other devices (how long this browser window waits) and Sounds
 * (this browser's notification sounds) are settings of a browser, kept in its storage, so the app leaves them out.
 */
export default function NotificationsScreen() {
  const prefs = useNotificationPrefs()
  const save = useSaveNotificationPrefs()
  const current = prefs.data
  const devices = usePushDevices()

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} onRefresh={() => Promise.all([prefs.refetch(), devices.refetch()])}>
      <SettingsCard title="This device" description="Get a notification here when you are not looking at Orbit.">
        <Text className="text-[14px] leading-[21px] text-muted-foreground">This app does not get notifications yet. Turn them on in Orbit in a browser to be notified there.</Text>
      </SettingsCard>
      <SettingsCard title="Devices" flush>
        <Devices />
      </SettingsCard>
      <SettingsCard title="Notify me about" description="These apply to all your devices.">
        {!current ? (
          prefs.isError ? (
            <View className="gap-2 items-start">
              <Note error>Your choices could not be loaded.</Note>
              <Button label="Retry" variant="ghost" onPress={() => void prefs.refetch()} />
            </View>
          ) : (
            <Note>Loading…</Note>
          )
        ) : (
          <View className="gap-4">
            <View>
              {NOTIFICATION_CHOICES.map(({ field, label }, index) => (
                <View key={field} className={`flex-row items-center justify-between gap-4 border-border ${index === 0 ? 'pt-0 border-t-0' : 'pt-3 border-t'} ${index === NOTIFICATION_CHOICES.length - 1 ? 'pb-0' : 'pb-3'}`}>
                  <Text className="font-noto-medium text-[14px] flex-1">{label}</Text>
                  <Toggle label={label} value={current[field]} onChange={(checked) => save.mutate({ ...current, [field]: checked })} />
                </View>
              ))}
            </View>
            <Text className="text-[12px] text-muted-foreground">Do not disturb turns all of these off. Muted channels never notify.</Text>
            {save.isError ? <Note error>Could not save your choice. {errorMessage(save.error)}</Note> : null}
          </View>
        )}
      </SettingsCard>
    </SettingsShell>
  )
}

/** The web's Devices card: each browser that gets notifications, with Remove. */
function Devices() {
  const devices = usePushDevices()
  const remove = useRemovePushDevice()
  const list = devices.data ?? []
  return (
    <>
      {devices.isPending ? <SettingsRow first><Note>Loading devices…</Note></SettingsRow> : null}
      {devices.isError ? (
        <SettingsRow first>
          <Note error>Devices could not be loaded.</Note>
          <Button label="Retry" variant="ghost" onPress={() => void devices.refetch()} />
        </SettingsRow>
      ) : null}
      {devices.isSuccess && list.length === 0 ? (
        <SettingsRow first>
          <Text className="flex-1 text-[14px] leading-[21px] text-muted-foreground">No device gets notifications yet.</Text>
        </SettingsRow>
      ) : null}
      {list.map((device, index) => (
        <SettingsRow key={device.id} first={index === 0}>
          <RowIcon icon={/Android|iOS|iPadOS/.test(device.label) ? 'mobile' : 'monitor'} />
          <RowText title={device.label} subtitle={`Added ${shortDate(device.created_at)}`} />
          <Button label="Remove" variant="ghost" disabled={remove.isPending} onPress={() => remove.mutate(device.id)} />
        </SettingsRow>
      ))}
      {remove.isError ? <SettingsRow><Note error>The device could not be removed. {errorMessage(remove.error)}</Note></SettingsRow> : null}
    </>
  )
}
