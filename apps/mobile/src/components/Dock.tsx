import { type Href, router } from 'expo-router'
import type { BottomTabBarProps } from 'expo-router/js-tabs'
import { Pressable, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { useChatBadge } from '../queries'
import { alpha, useTheme } from '../theme'
import { CountBadge, Icon, type IconName, Text } from '../ui'

/** The web `MobileDock`: the tab routes in its order, Mail disabled as "coming soon". `route` is null for Mail. */
const ITEMS: { route: string | null; href: Href; label: string; icon: IconName }[] = [
  { route: 'index', href: '/', label: 'Home', icon: 'home' },
  { route: 'tasks', href: '/tasks', label: 'Tasks', icon: 'tasks' },
  { route: 'docs', href: '/docs', label: 'Docs', icon: 'docs' },
  { route: 'chat', href: '/chat', label: 'Chat', icon: 'chat' },
  { route: null, href: '/', label: 'Mail', icon: 'mail' },
  { route: 'settings', href: '/settings', label: 'Settings', icon: 'settings' },
]

export function Dock({ state }: BottomTabBarProps) {
  const { colors } = useTheme()
  const insets = useSafeAreaInsets()
  const chatBadge = useChatBadge()
  const current = state.routes[state.index]?.name
  return (
    <View accessibilityRole="tablist" className="flex-row min-h-14 pt-3 bg-background" style={{ paddingBottom: insets.bottom }}>
      {ITEMS.map((item) => {
        const enabled = item.route !== null
        const active = enabled && item.route === current
        const count = item.route === 'chat' ? chatBadge.count : 0
        // a dot when chat is unread but nothing counts, like a channel message that does not mention you
        const dot = item.route === 'chat' && count === 0 && chatBadge.unread
        const color = active ? colors.primary : enabled ? colors.mutedForeground : alpha(colors.mutedForeground, 0.5)
        return (
          <Pressable
            key={item.label}
            accessibilityRole="tab"
            accessibilityLabel={count > 0 ? `${item.label}, ${count} unread` : dot ? `${item.label}, unread` : item.label}
            accessibilityState={{ selected: active, disabled: !enabled }}
            disabled={!enabled}
            onPress={() => router.navigate(item.href)}
            className="flex-1 h-11 items-center justify-center gap-0.5"
          >
            <View>
              <Icon name={item.icon} size={20} color={color} />
              {count > 0 ? (
                <View className="absolute -top-1 left-3">
                  <CountBadge count={count} />
                </View>
              ) : dot ? (
                <View className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-primary" />
              ) : null}
            </View>
            <Text className={`font-noto-medium text-[10px] ${active ? 'text-primary' : enabled ? 'text-muted-foreground' : 'text-muted-foreground/50'}`}>{item.label}</Text>
          </Pressable>
        )
      })}
    </View>
  )
}
