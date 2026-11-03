import { View } from 'react-native'

import { ACCOUNT_NAV, Note, SettingsCard, SettingsShell, Toggle } from '../../../components/settings/Shell'
import { NOTIFICATION_CHOICES } from '../../../data/account-api'
import { useNotificationPrefs, useSaveNotificationPrefs } from '../../../hooks/account'
import { errorMessage } from '../../../session-context'
import { Button, Text } from '../../../ui'

/**
 * The web's Notifications page, the part that is the account's: what Orbit notifies about. This device, Devices,
 * Other devices and Sounds are about the browser's web push and sounds; the app has no push notifications yet.
 */
export default function NotificationsScreen() {
  const prefs = useNotificationPrefs()
  const save = useSaveNotificationPrefs()
  const current = prefs.data

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} refreshing={prefs.isRefetching} onRefresh={() => void prefs.refetch()}>
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
