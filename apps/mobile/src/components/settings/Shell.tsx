import { type Href, router, useNavigation, usePathname } from 'expo-router'
import type { Permission } from '@orbit/api/generated/types.gen'
import type { ReactNode } from 'react'
import { KeyboardAvoidingView, Platform, Pressable, RefreshControl, ScrollView, Switch, View } from 'react-native'

import { useOrbit } from '../../orbit-context'
import { alpha, cssColors, setSchemePreference, useLayout, useTheme } from '../../theme'
import { Icon, type IconName, IconButton, PaneHeader, SafeAreaView, Text } from '../../ui'
import { MenuButton } from '../Drawer'

export interface SettingsNavItem {
  href: Href & string
  label: string
  icon: IconName
  /** Shown only to roles that hold this permission. */
  permission?: Permission
}

/**
 * The web's settings areas at phone size. API tokens, Emoji, Stickers, GitHub and Billing are desktop setup work, and
 * Keyboard shortcuts and Storage (this browser's chat cache) have no phone counterpart, so they are left out.
 */
export const WORKSPACE_NAV: readonly SettingsNavItem[] = [
  { href: '/settings', label: 'General', icon: 'settings' },
  { href: '/settings/file-storage', label: 'File storage', icon: 'cloudConnection', permission: 'storage.manage' },
  { href: '/settings/members', label: 'Members', icon: 'people' },
]

/** The links of `nav` this workspace role may open. */
export function allowedNav<T extends SettingsNavItem>(nav: readonly T[], permissions: readonly Permission[]): T[] {
  return nav.filter((item) => !item.permission || permissions.includes(item.permission))
}

export const ACCOUNT_NAV: readonly SettingsNavItem[] = [
  { href: '/settings/profile', label: 'Profile', icon: 'user' },
  { href: '/settings/appearance', label: 'Appearance', icon: 'brush' },
  { href: '/settings/notifications', label: 'Notifications', icon: 'bell' },
  { href: '/settings/security', label: 'Security', icon: 'lock' },
  { href: '/settings/sessions', label: 'Sessions', icon: 'shield' },
  { href: '/settings/agentic', label: 'Agentic', icon: 'robot' },
]

/** The web's Admin pages in its order; General is the area's first page, as `/admin` is on the web. */
export const ADMIN_NAV: readonly SettingsNavItem[] = [
  { href: '/settings/admin', label: 'General', icon: 'settings' },
  { href: '/settings/admin/users', label: 'Users', icon: 'people' },
  { href: '/settings/admin/email', label: 'Email', icon: 'mail' },
  { href: '/settings/admin/limits', label: 'Limits', icon: 'chart' },
  { href: '/settings/admin/storage', label: 'Storage', icon: 'cloudConnection' },
  { href: '/settings/admin/backups', label: 'Backups', icon: 'archiveBox' },
  { href: '/settings/admin/audit', label: 'Audit log', icon: 'clipboardText' },
  { href: '/settings/admin/jobs', label: 'Background jobs', icon: 'cpu' },
]

/**
 * The web's `SettingsShell` on a phone: the topbar, the two-column sub-navigation between hairlines, then the cards.
 * A page opened from another screen gets a back button; the area's own pages swap in place. In the new UI the drawer
 * lists the area's pages instead, so the grid is left out.
 */
export function SettingsShell({ title, nav, children, refreshing, onRefresh }: { title: string; nav: readonly SettingsNavItem[]; children: ReactNode; refreshing?: boolean; onRefresh?: () => void }) {
  const { colors, scheme } = useTheme()
  const navigation = useNavigation()
  const pathname = usePathname()
  const newUi = useLayout() === 'new'
  const { workspace } = useOrbit()
  const muted = alpha(colors.mutedForeground, 0.7)
  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader>
        {/* only this stack's history: the tab navigator around it can always go back */}
        {(navigation.getState()?.index ?? 0) > 0 ? <IconButton icon="back" label="Back" color={muted} onPress={() => navigation.goBack()} /> : <MenuButton />}
        <Text numberOfLines={1} className="font-noto-medium text-[13.5px] flex-1">
          {title}
        </Text>
        <View className="flex-row gap-1.5">
          <IconButton icon="search" label="Search" color={muted} onPress={() => router.navigate('/tasks?search=1')} />
          <IconButton icon={scheme === 'dark' ? 'sun' : 'moon'} label="Toggle theme" iconSize={15} color={muted} onPress={() => setSchemePreference(scheme === 'dark' ? 'light' : 'dark')} />
        </View>
      </PaneHeader>
      <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <ScrollView
          contentContainerClassName="px-5 pt-5 pb-8 gap-6"
          keyboardShouldPersistTaps="handled"
          refreshControl={onRefresh ? <RefreshControl refreshing={refreshing ?? false} onRefresh={onRefresh} tintColor={colors.mutedForeground} /> : undefined}
        >
          {newUi ? null : (
            <View accessibilityRole="tablist" accessibilityLabel={title} className="flex-row flex-wrap gap-0.5 py-3 border-t border-b border-border">
              {allowedNav(nav, workspace.permissions).map((item) => {
                const active = pathname === item.href
                return (
                  <Pressable
                    key={item.href}
                    accessibilityRole="tab"
                    accessibilityLabel={item.label}
                    accessibilityState={{ selected: active }}
                    onPress={active ? undefined : () => router.replace(item.href)}
                    className={`w-[49.6%] h-8 flex-row items-center gap-2.5 px-2.5 rounded-lg ${active ? 'bg-sidebar-accent' : 'active:bg-muted'}`}
                  >
                    <Icon name={item.icon} size={18} color={alpha(active ? colors.sidebarAccentForeground : colors.foreground, 0.9)} />
                    <Text numberOfLines={1} className="font-noto-medium text-[13px] flex-1">
                      {item.label}
                    </Text>
                  </Pressable>
                )
              })}
            </View>
          )}
          {children}
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView>
  )
}

/** The Admin area: only instance admins see it (the server refuses the others anyway). */
export function AdminShell({ children, refreshing, onRefresh }: { children: ReactNode; refreshing?: boolean; onRefresh?: () => void }) {
  const { user } = useOrbit()
  return (
    <SettingsShell title="Admin" nav={ADMIN_NAV} refreshing={refreshing} onRefresh={onRefresh}>
      {user.installation_admin ? children : <Note>Only the instance admins of this Orbit server see the Admin area.</Note>}
    </SettingsShell>
  )
}

/** The web `SettingsCard`: a muted title strip (with its actions) on the card surface, the body on an inset panel. */
export function SettingsCard({ title, description, actions, flush = false, children }: { title: string; description?: string; actions?: ReactNode; flush?: boolean; children?: ReactNode }) {
  return (
    <View className="rounded-lg bg-card shadow-[0_0_0_1px_var(--color-border)]">
      <View className="min-h-12 justify-center gap-2 py-2.5 pr-2 pl-4">
        <View>
          <Text className="font-noto-medium text-[14px] leading-[20px] text-muted-foreground">{title}</Text>
          {description ? <Text className="font-noto-medium text-[12px] leading-[16px] mt-0.5 text-muted-foreground">{description}</Text> : null}
        </View>
        {actions ? <View className="flex-row flex-wrap gap-2">{actions}</View> : null}
      </View>
      {children !== undefined ? (
        <View className={`rounded-lg overflow-hidden bg-background shadow-[0_0_0_1px_var(--color-muted)] ${flush ? 'p-0' : 'p-4'}`}>{children}</View>
      ) : null}
    </View>
  )
}

/** One row of a flush card; rows after the first get the hairline above. Only a row with `onPress` is a button. */
export function SettingsRow({ children, first = false, onPress, label }: { children: ReactNode; first?: boolean; onPress?: () => void; label?: string }) {
  const className = `min-h-12 flex-row items-center gap-3 border-border px-4 py-3 ${first ? '' : 'border-t'}`
  if (!onPress) return <View className={className}>{children}</View>
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} onPress={onPress} className={`${className} active:bg-muted`}>
      {children}
    </Pressable>
  )
}

/** The square icon tile at the start of a settings row. */
export function RowIcon({ icon }: { icon: IconName }) {
  const { colors } = useTheme()
  return (
    <View className="size-9 rounded-md bg-muted items-center justify-center">
      <Icon name={icon} size={18} color={colors.mutedForeground} />
    </View>
  )
}

/** A row's title and its quiet second line. */
export function RowText({ title, subtitle, badges }: { title: string; subtitle?: string; badges?: ReactNode }) {
  return (
    <View className="flex-1 min-w-0 gap-0.5">
      <View className="flex-row items-center gap-1.5 flex-wrap">
        <Text numberOfLines={1} className="font-noto-medium text-[13px] shrink">
          {title}
        </Text>
        {badges}
      </View>
      {subtitle ? (
        <Text numberOfLines={2} className="text-[12px] text-muted-foreground/85">
          {subtitle}
        </Text>
      ) : null}
    </View>
  )
}

/** A switch row's title and its explanation lines, which may wrap (unlike `RowText`'s two-line subtitle). */
export function OptionText({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <View className="flex-1 min-w-0 gap-0.5">
      <Text className="font-noto-medium text-[13px]">{title}</Text>
      {children}
    </View>
  )
}

/** One quiet line under an `OptionText` title; `error` for a failure. */
export function OptionHint({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <Text className={`text-[12px] leading-[17px] ${error ? 'text-destructive' : 'text-muted-foreground'}`}>{children}</Text>
}

/** A form's label above a control that is not an `Input` (a `Segmented`, a switch). */
export function FieldLabel({ children }: { children: ReactNode }) {
  return <Text className="font-noto-medium text-[14px] leading-[19.25px]">{children}</Text>
}

export type BadgeTone = 'primary' | 'secondary' | 'outline' | 'destructive'

/** The web `Badge`: a 20px pill. */
export function Badge({ label, tone = 'primary' }: { label: string; tone?: BadgeTone }) {
  return (
    <View className={`h-5 justify-center rounded-full border px-2 ${BADGE[tone].box}`}>
      <Text className={`font-noto-medium text-[12px] leading-[16px] ${BADGE[tone].text}`}>{label}</Text>
    </View>
  )
}

const BADGE: Record<BadgeTone, { box: string; text: string }> = {
  primary: { box: 'border-transparent bg-primary', text: 'text-primary-foreground' },
  secondary: { box: 'border-transparent bg-secondary', text: 'text-secondary-foreground' },
  outline: { box: 'border-border', text: 'text-foreground' },
  destructive: { box: 'border-transparent bg-destructive/10 dark:bg-destructive/20', text: 'text-destructive' },
}

/** The web `Switch` in the theme's colors. */
export function Toggle({ value, label, onChange, disabled }: { value: boolean; label: string; onChange: (value: boolean) => void; disabled?: boolean }) {
  const { colors } = useTheme()
  return (
    <Switch
      accessibilityLabel={label}
      value={value}
      disabled={disabled}
      onValueChange={onChange}
      trackColor={{ false: colors.input, true: colors.primary }}
      thumbColor={cssColors.white}
      ios_backgroundColor={colors.input}
      // react-native-web paints the on thumb teal unless told otherwise
      {...({ activeThumbColor: cssColors.white } as object)}
    />
  )
}

/** A choice between a few options in one row (the web's small `Select`s). */
export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: readonly { value: T; label: string }[]; onChange: (value: T) => void; label: string }) {
  const { colors } = useTheme()
  return (
    <View accessibilityRole="radiogroup" accessibilityLabel={label} className="flex-row p-0.5 gap-0.5 rounded-lg bg-muted">
      {options.map((option) => {
        const selected = option.value === value
        return (
          <Pressable
            key={option.value}
            accessibilityRole="radio"
            accessibilityLabel={option.label}
            accessibilityState={{ selected }}
            onPress={() => onChange(option.value)}
            className={`flex-1 h-7 items-center justify-center rounded-md ${selected ? 'bg-background' : ''}`} style={{ boxShadow: selected ? `0 1px 2px ${alpha(colors.foreground, 0.12)}` : undefined }}
          >
            <Text className={`font-noto-medium text-[13px] ${selected ? 'text-foreground' : 'text-muted-foreground'}`}>{option.label}</Text>
          </Pressable>
        )
      })}
    </View>
  )
}

/** A quiet status or error line inside a card. */
export function Note({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <Text className={`text-[13px] leading-[19px] ${error ? 'text-destructive' : 'text-muted-foreground'}`}>{children}</Text>
}
