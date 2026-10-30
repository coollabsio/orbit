import { Tabs } from 'expo-router/js-tabs'

import { BillingBanner } from '../../components/BillingBanner'
import { Dock } from '../../components/Dock'
import { DrawerProvider } from '../../components/Drawer'
import { MailServerBanner } from '../../components/MailServerBanner'
import { useLiveUpdates } from '../../live'
import { useTheme } from '../../theme'

export default function TabsLayout() {
  const { colors } = useTheme()
  useLiveUpdates()
  return (
    <DrawerProvider>
      <Tabs tabBar={(props) => (
          <>
            <MailServerBanner />
            <BillingBanner />
            <Dock {...props} />
          </>
        )} screenOptions={{ headerShown: false, sceneStyle: { backgroundColor: colors.background } }}>
        <Tabs.Screen name="index" />
        <Tabs.Screen name="tasks" />
        <Tabs.Screen name="docs" />
        <Tabs.Screen name="chat" />
        <Tabs.Screen name="settings" />
        {/* reached from the drawer, like the web's Activity: no dock item */}
        <Tabs.Screen name="activity" />
      </Tabs>
    </DrawerProvider>
  )
}
