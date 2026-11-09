import { useEffect, useState } from 'react'
import { AppState, Linking, View } from 'react-native'

import { ACCOUNT_NAV, Badge, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell, Toggle } from '../../../components/settings/Shell'
import { NOTIFICATION_CHOICES } from '../../../data/account-api'
import { shortDate } from '../../../data/format'
import { useApnsDevices, useNotificationPrefs, usePushDevices, useRemovePushDevice, useSaveNotificationPrefs, useSendTestPush } from '../../../hooks/account'
import { type ThisDevice as ThisDeviceState, useRemoveApnsDevice, useThisDevice, useTurnOnPush } from '../../../push'
import { errorMessage } from '../../../session-context'
import { Button, Text } from '../../../ui'

/**
 * The web's Notifications page. This device turns on iOS notifications (APNs) for this iPhone; Devices lists the
 * browsers and the iOS devices that get them, from the server. Other devices (how long this browser window waits) and
 * Sounds (this browser's notification sounds) are settings of a browser, kept in its storage, so the app leaves them out.
 */
export default function NotificationsScreen() {
  const prefs = useNotificationPrefs()
  const save = useSaveNotificationPrefs()
  const current = prefs.data
  const devices = usePushDevices()
  const apns = useApnsDevices()
  const here = useThisDevice({ refresh: true })

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} onRefresh={() => Promise.all([prefs.refetch(), devices.refetch(), apns.refetch(), here.refetch()])}>
      <SettingsCard title="This device" description="Get a notification here when you are not looking at Orbit.">
        <ThisDevice />
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

const PARAGRAPH = 'text-[14px] leading-[21px] text-muted-foreground'

const STATE_TEXT: Record<Exclude<ThisDeviceState['state'], 'unsupported'>, string> = {
  on: 'Notifications are on for this device.',
  off: 'Notifications are off on this device.',
  denied: 'Notifications are off for Orbit in iOS Settings. Turn on Allow Notifications there, then come back.',
  not_configured: 'This Orbit server is not set up to send notifications to the app. Ask your admin to add an APNs key.',
}

/** Whether this iPhone gets notifications, with the button that changes it, as the web's This device. */
function ThisDevice() {
  const device = useThisDevice({ refresh: true })
  const turnOn = useTurnOnPush()
  const test = useSendTestPush()
  const [sent, setSent] = useState<string | null>(null)
  const refetch = device.refetch

  // back from iOS Settings: the permission may have changed
  useEffect(() => {
    const subscription = AppState.addEventListener('change', (next) => {
      if (next === 'active') void refetch()
    })
    return () => subscription.remove()
  }, [refetch])

  const state = device.data
  if (!state) {
    if (device.isError) {
      return (
        <View className="gap-2 items-start">
          <Note error>Could not check this device. {errorMessage(device.error)}</Note>
          <Button label="Retry" variant="ghost" onPress={() => void device.refetch()} />
        </View>
      )
    }
    return <Note>Checking this device…</Note>
  }
  if (state.state === 'unsupported') {
    return (
      <Text className={PARAGRAPH}>
        {state.reason === 'expo-go'
          ? 'Expo Go cannot get Orbit notifications. Install Orbit from TestFlight or the App Store to get them here.'
          : 'This app gets notifications on iPhone and iPad only. Turn them on in Orbit in a browser to be notified there.'}
      </Text>
    )
  }

  const sendTest = () =>
    test.mutate(undefined, {
      onSuccess: (count) => setSent(`Sent to ${count} ${count === 1 ? 'device' : 'devices'}.`),
      onError: (error) => setSent(`Could not send the test notification. ${errorMessage(error)}`),
    })

  return (
    <View className="gap-3">
      <Text className={PARAGRAPH}>{STATE_TEXT[state.state]}</Text>
      <View className="flex-row flex-wrap items-center gap-2">
        {state.state === 'off' ? <Button label="Turn on notifications" loading={turnOn.isPending} onPress={() => turnOn.mutate()} /> : null}
        {state.state === 'denied' ? <Button label="Open Settings" variant="outline" onPress={() => void Linking.openSettings()} /> : null}
        {state.state === 'on' ? <Button label="Send a test notification" variant="outline" loading={test.isPending} onPress={sendTest} /> : null}
      </View>
      {turnOn.isError ? <Note error>Could not turn on notifications. {errorMessage(turnOn.error)}</Note> : null}
      {sent ? <Note error={test.isError}>{sent}</Note> : null}
    </View>
  )
}

/** The web's Devices card: the browsers and the iOS devices that get notifications, each with Remove. */
function Devices() {
  const browsers = usePushDevices()
  const apns = useApnsDevices()
  const here = useThisDevice()
  const removeBrowser = useRemovePushDevice()
  const removeApns = useRemoveApnsDevice()
  const hereId = here.data?.state === 'on' ? here.data.deviceId : null
  const list = [
    ...(apns.data ?? []).map((device) => ({ ...device, mobile: true, remove: () => removeApns.mutate(device.id) })),
    ...(browsers.data ?? []).map((device) => ({ ...device, mobile: /Android|iOS|iPadOS/.test(device.label), remove: () => removeBrowser.mutate(device.id) })),
  ]
  const pending = browsers.isPending || apns.isPending
  const failed = browsers.isError || apns.isError
  const removeError = removeBrowser.error ?? removeApns.error
  return (
    <>
      {pending ? <SettingsRow first><Note>Loading devices…</Note></SettingsRow> : null}
      {failed ? (
        <SettingsRow first>
          <Note error>Devices could not be loaded.</Note>
          <Button label="Retry" variant="ghost" onPress={() => void Promise.all([browsers.refetch(), apns.refetch()])} />
        </SettingsRow>
      ) : null}
      {!pending && !failed && list.length === 0 ? (
        <SettingsRow first>
          <Text className="flex-1 text-[14px] leading-[21px] text-muted-foreground">No device gets notifications yet.</Text>
        </SettingsRow>
      ) : null}
      {!pending && !failed
        ? list.map((device, index) => (
            <SettingsRow key={device.id} first={index === 0}>
              <RowIcon icon={device.mobile ? 'mobile' : 'monitor'} />
              <RowText title={device.label} subtitle={`Added ${shortDate(device.created_at)}`} badges={device.id === hereId ? <Badge label="This device" /> : undefined} />
              <Button label="Remove" variant="ghost" disabled={removeBrowser.isPending || removeApns.isPending} onPress={device.remove} />
            </SettingsRow>
          ))
        : null}
      {removeError ? <SettingsRow><Note error>The device could not be removed. {errorMessage(removeError)}</Note></SettingsRow> : null}
    </>
  )
}
