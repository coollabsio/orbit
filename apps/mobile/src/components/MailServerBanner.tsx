import { router } from 'expo-router'
import { Pressable, View } from 'react-native'

import { MAIL_SERVER_WARNING, mailServerMissing } from '../data/mail-server'
import { useAuthOptions } from '../hooks/account'
import { useOrbit } from '../orbit-context'
import { Text } from '../ui'

/** The web's `MailServerBanner`: above the dock on every tab while Orbit Cloud has no mail server, for installation admins. */
export function MailServerBanner() {
  const { user } = useOrbit()
  const options = useAuthOptions().data
  if (!mailServerMissing(options, user.installation_admin)) return null
  return (
    <View accessibilityRole="alert" className="flex-row flex-wrap items-center justify-center gap-x-2 border-t border-destructive/20 bg-destructive/10 px-3 py-1 dark:bg-destructive/20">
      <Text className="text-center font-noto-medium text-[12px] text-destructive">{MAIL_SERVER_WARNING}</Text>
      <Pressable accessibilityRole="link" hitSlop={8} onPress={() => router.navigate('/settings/admin/email')}>
        <Text className="font-noto-semibold text-[12px] text-destructive underline">Set it in Admin → Email</Text>
      </Pressable>
    </View>
  )
}
