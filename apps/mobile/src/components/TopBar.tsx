import { router, usePathname } from 'expo-router'
import { useState } from 'react'
import { Pressable, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { SvgXml } from 'react-native-svg'

import { isGuestRole } from '../data/guest'
import { useOrbit } from '../orbit-context'
import { useUnreadNotifications } from '../queries'
import { alpha, useTheme } from '../theme'
import { Avatar, Icon, PickerSheet, Text } from '../ui'
import { logo } from '../ui/icons.gen'
import { AccountMenu } from './AccountMenu'
import { useOpenDrawer } from './Drawer'

/**
 * The web's phone `Topbar`: logo, workspace switcher, Activity, account menu and the button that opens the drawer.
 * Screens keep their own headers below it.
 */
export function TopBar() {
  const { colors, scheme } = useTheme()
  const insets = useSafeAreaInsets()
  const openDrawer = useOpenDrawer()
  const { user, workspace, workspaces, selectWorkspace } = useOrbit()
  const pathname = usePathname()
  // a guest has Chat and its account settings only: no Activity
  const guest = isGuestRole(workspace.role)
  const unread = (useUnreadNotifications(!guest).data?.length ?? 0) > 0
  const [switching, setSwitching] = useState(false)
  const [accountOpen, setAccountOpen] = useState(false)
  const onActivity = pathname === '/activity'
  const muted = alpha(colors.mutedForeground, 0.7)
  const dark = scheme === 'dark'

  return (
    <View className="flex-row items-center justify-between gap-4 border-b border-border bg-card px-4 py-3" style={{ paddingTop: 12 + insets.top, paddingLeft: 16 + insets.left, paddingRight: 16 + insets.right }}>
      <View className="min-w-0 flex-1 flex-row items-center gap-2.5">
        <Pressable accessibilityRole="link" accessibilityLabel="Home" onPress={() => router.navigate('/')} className="size-8 items-center justify-center rounded-lg bg-muted active:opacity-80">
          <SvgXml xml={logo} width={18} height={18} />
        </Pressable>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Workspace: ${workspace.name}`}
          onPress={() => setSwitching(true)}
          className="-ml-1 h-8 min-w-0 shrink-0 flex-row items-center gap-1.5 rounded-lg px-2 active:bg-muted"
        >
          <View className="size-5 items-center justify-center rounded-md bg-muted">
            <Text className="font-noto-semibold text-[11px] text-muted-foreground">{workspace.name.charAt(0).toUpperCase()}</Text>
          </View>
          <Text numberOfLines={1} className="shrink font-noto-semibold text-[13px]">
            {workspace.name}
          </Text>
          <Icon name="chevronExpandY" size={16} color={muted} />
        </Pressable>
      </View>
      <View className="flex-row items-center gap-1">
        {guest ? null : (
          <Pressable
            accessibilityRole="link"
            accessibilityLabel={unread ? 'Activity, unread' : 'Activity'}
            accessibilityState={{ selected: onActivity }}
            onPress={() => router.navigate('/activity')}
            className={`size-9 items-center justify-center rounded-lg active:bg-muted ${onActivity ? 'bg-muted' : ''}`}
          >
            <Icon name="bell" size={20} color={onActivity ? colors.foreground : colors.mutedForeground} />
            {unread ? <View className="absolute top-1.5 right-1.5 size-3 rounded-full border-2 border-card bg-primary" /> : null}
          </Pressable>
        )}
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Account menu for ${user.display_name}`}
          onPress={() => setAccountOpen(true)}
          // one set of classes per scheme, as in the drawer: Uniwind does not let `dark:border-*` win over `border-border`
          className={`h-9 flex-row items-center gap-1.5 rounded-lg border px-2 ${dark ? 'border-sidebar-tint/24 bg-primary/14 active:bg-primary/18' : 'border-border bg-background active:bg-muted'}`}
        >
          <Avatar id={user.id} name={user.display_name} size={24} presence={user.status.presence} />
          <Icon name="chevronDown" size={14} color={dark ? colors.sidebarTint : muted} />
        </Pressable>
        <Pressable accessibilityRole="button" accessibilityLabel="Open sidebar" onPress={openDrawer} className="-mr-1 size-9 items-center justify-center rounded-lg active:bg-muted">
          <Icon name="sidebarRight2" size={20} color={colors.mutedForeground} />
        </Pressable>
      </View>
      <AccountMenu visible={accountOpen} onClose={() => setAccountOpen(false)} onNavigate={(href) => router.navigate(href)} />
      <PickerSheet
        visible={switching}
        title="Workspaces"
        options={workspaces.map((item) => ({ key: item.id, label: item.name, subtitle: item.role, selected: item.id === workspace.id }))}
        onSelect={(id) => {
          setSwitching(false)
          selectWorkspace(id)
        }}
        onClose={() => setSwitching(false)}
      />
    </View>
  )
}
