import { type Href, usePathname } from 'expo-router'
import type { Permission } from '@orbit/api/generated/types.gen'
import { type ReactNode, useState } from 'react'
import { KeyboardAvoidingView, Platform, Pressable, ScrollView, Switch, View } from 'react-native'

import { useOrbit } from '../../orbit-context'
import { alpha, cssColors, useButtonStyle, useTheme } from '../../theme'
import { Icon, type IconName, PaneHeader, PickerSheet, type Presence, PullRefresh, SafeAreaView, StatusMark, Text } from '../../ui'

export interface SettingsNavItem {
  href: Href & string
  label: string
  icon: IconName
  /** Shown only to roles that hold this permission. */
  permission?: Permission
  /** Shown only in Orbit Cloud (billing on). */
  cloud?: boolean
}

/**
 * The web's workspace settings in its order; `drawerAreas.ts` groups them under the web's section labels. Agentic
 * lives at `/settings/workspace-agentic` because `/settings/agentic` is the account's.
 */
export const WORKSPACE_NAV: readonly SettingsNavItem[] = [
  { href: '/settings', label: 'General', icon: 'settings' },
  { href: '/settings/members', label: 'Members', icon: 'people' },
  { href: '/settings/billing', label: 'Billing', icon: 'card', cloud: true },
  { href: '/settings/emoji', label: 'Emoji', icon: 'smile', permission: 'chat.manage' },
  { href: '/settings/stickers', label: 'Stickers', icon: 'sticker', permission: 'chat.manage' },
  { href: '/settings/file-storage', label: 'File storage', icon: 'cloudConnection' },
  { href: '/settings/github', label: 'GitHub', icon: 'link2' },
  { href: '/settings/workspace-agentic', label: 'Agentic', icon: 'robot' },
  { href: '/settings/api-tokens', label: 'API tokens', icon: 'key', permission: 'api_tokens.manage' },
  { href: '/settings/import', label: 'Import', icon: 'import' },
  { href: '/settings/export', label: 'Export', icon: 'documentDownload', permission: 'workspace.export' },
  { href: '/settings/danger-zone', label: 'Danger zone', icon: 'danger', permission: 'workspace.delete' },
]

/** The links of `nav` this workspace role may open; the cloud-only ones only in Orbit Cloud. */
export function allowedNav<T extends SettingsNavItem>(nav: readonly T[], permissions: readonly Permission[], cloud: boolean): T[] {
  return nav.filter((item) => (!item.permission || permissions.includes(item.permission)) && (!item.cloud || cloud))
}

export const ACCOUNT_NAV: readonly SettingsNavItem[] = [
  { href: '/settings/profile', label: 'Profile', icon: 'user' },
  { href: '/settings/security', label: 'Security', icon: 'lock' },
  { href: '/settings/sessions', label: 'Sessions', icon: 'shield' },
  { href: '/settings/appearance', label: 'Appearance', icon: 'brush' },
  { href: '/settings/notifications', label: 'Notifications', icon: 'bell' },
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
 * The web's `SettingsShell` on a phone: the topbar, then the cards. No back button: the drawer lists the area's pages,
 * and the dock leaves the area.
 */
export function SettingsShell({ title, nav, children, onRefresh }: { title: string; nav: readonly SettingsNavItem[]; children: ReactNode; onRefresh?: () => Promise<unknown> }) {
  const { colors } = useTheme()
  const pathname = usePathname()
  // the longest matching link names the page, so `/settings/admin/users` beats `/settings/admin`
  const page = nav
    .filter((item) => pathname === item.href || pathname.startsWith(`${item.href}/`))
    .sort((a, b) => b.href.length - a.href.length)[0]
  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      {/* the web's breadcrumb header: the area (muted, with its icon) / the page; same left inset as the cards */}
      <PaneHeader className="min-h-12 gap-[7px] pl-5">
        <Icon name={AREA_ICON[title] ?? 'settings'} size={15} color={colors.mutedForeground} />
        <Text numberOfLines={1} className="shrink font-noto-semibold text-[13px] text-muted-foreground">
          {title}
        </Text>
        {page ? (
          <>
            <Text className="text-[13px] text-muted-foreground">/</Text>
            <Text accessibilityRole="header" numberOfLines={1} className="shrink font-noto-semibold text-[13px] text-foreground">
              {page.label}
            </Text>
          </>
        ) : null}
      </PaneHeader>
      <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <ScrollView
          contentContainerClassName="px-5 pt-5 pb-8 gap-6"
          keyboardShouldPersistTaps="handled"
          refreshControl={onRefresh ? <PullRefresh onRefresh={onRefresh} /> : undefined}
        >
          {children}
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView>
  )
}

/** The web areas' header icons (`settingsNav.ts`). */
const AREA_ICON: Record<string, IconName> = { Settings: 'settings', 'Account settings': 'user', Admin: 'admin' }

/** The Admin area: only instance admins see it (the server refuses the others anyway). */
export function AdminShell({ children, onRefresh }: { children: ReactNode; onRefresh?: () => Promise<unknown> }) {
  const { user } = useOrbit()
  return (
    <SettingsShell title="Admin" nav={ADMIN_NAV} onRefresh={onRefresh}>
      {user.installation_admin ? children : <Note>Only the instance admins of this Orbit server see the Admin area.</Note>}
    </SettingsShell>
  )
}

/**
 * The web `SettingsCard`: a title strip (with its actions on the right) on the card surface, the body on a panel below
 * a hairline. A description hides behind the dotted-underlined title (the web's tooltip); tapping the title shows it.
 */
export function SettingsCard({ title, description, actions, flush = false, danger = false, children }: { title: string; description?: string; actions?: ReactNode; flush?: boolean; /** Delete account: a destructive ring and title, and a body without its own fill. */ danger?: boolean; children?: ReactNode }) {
  const { colors } = useTheme()
  const [open, setOpen] = useState(false)
  return (
    <View className={`rounded-lg bg-card ${danger ? 'shadow-[0_0_0_1px_var(--color-destructive)]' : 'shadow-[0_0_0_1px_var(--color-border)]'}`}>
      <View className="min-h-12 justify-center py-2 pr-2 pl-4">
        <View className="flex-row flex-wrap items-center justify-between gap-2">
          <View className="min-w-0 shrink py-0.5">
            <Text
              accessibilityRole={description ? 'button' : 'header'}
              accessibilityHint={description}
              onPress={description ? () => setOpen((value) => !value) : undefined}
              className={`font-noto-medium text-[14px] leading-[20px] ${danger ? 'text-destructive' : 'text-foreground'} ${description ? 'underline' : ''}`}
              // dotted and muted like the web's `decoration-dotted decoration-muted-foreground` (no class for these)
              style={description ? { textDecorationStyle: 'dotted', textDecorationColor: colors.mutedForeground } : undefined}
            >
              {title}
            </Text>
          </View>
          {actions ? <View className="min-w-0 max-w-full flex-row flex-wrap items-center gap-2">{actions}</View> : null}
        </View>
        {/* the web shows it in a tooltip; a tap on the title shows it here, under the whole row */}
        {description && open ? <Text className="pr-2 pb-0.5 text-[12px] leading-[16px] text-muted-foreground">{description}</Text> : null}
      </View>
      {children !== undefined ? (
        <View className={`rounded-b-lg overflow-hidden border-t border-border ${danger ? '' : 'bg-background'} ${flush ? 'p-0' : 'p-4'}`}>{children}</View>
      ) : null}
    </View>
  )
}

/** The web `Select` (its 32px trigger, or the 28px `sm` one): the current choice and a chevron; the choices open in a `PickerSheet`. */
export function Select<T extends string>({ value, options, onChange, label, size = 'default' }: { value: T; options: readonly { value: T; label: string }[]; onChange: (value: T) => void; label: string; size?: 'default' | 'sm' }) {
  const { colors } = useTheme()
  const [open, setOpen] = useState(false)
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={label}
        onPress={() => setOpen(true)}
        className={`flex-row items-center justify-between gap-1.5 border border-input pr-2 pl-2.5 dark:bg-input/30 active:bg-input/50 ${size === 'sm' ? 'h-7 rounded-md' : 'h-8 rounded-lg'}`}
      >
        <Text numberOfLines={1} className={`shrink text-foreground ${size === 'sm' ? 'text-[13px]' : 'text-[14px]'}`}>
          {options.find((option) => option.value === value)?.label}
        </Text>
        <Icon name="chevronDown" size={16} color={colors.mutedForeground} />
      </Pressable>
      <PickerSheet
        visible={open}
        title={label}
        options={options.map((option) => ({ key: option.value, label: option.label, selected: option.value === value }))}
        onSelect={(key) => {
          setOpen(false)
          onChange(key as T)
        }}
        onClose={() => setOpen(false)}
      />
    </>
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

/**
 * The settings pages' avatar on the web (`Avatar size="sm"` + `AvatarFallback rounded-lg text-[11px]`): one initial
 * on the muted fill, with the hairline ring, and an optional presence badge.
 */
export function InitialAvatar({ name, size = 28, presence }: { name: string; size?: number; presence?: Presence }) {
  const { colors } = useTheme()
  const badge = size <= 24 ? 8 : size <= 40 ? 10 : 12
  return (
    <View accessibilityLabel={name} style={{ width: size, height: size }}>
      <View className="items-center justify-center rounded-lg bg-muted" style={{ width: size, height: size }}>
        <Text className="font-noto-semibold text-[11px] text-muted-foreground">{name.charAt(0).toUpperCase()}</Text>
      </View>
      <View className="absolute inset-0 rounded-full border border-border" />
      {presence ? (
        <View className="absolute -right-0.5 -bottom-0.5 rounded-full p-0.5 bg-background">
          <StatusMark presence={presence} size={badge} surface={colors.background} />
        </View>
      ) : null}
    </View>
  )
}

/**
 * The web's 28px outline button with 11px text: the members' and users' `Manage`, and the `Copy` on a code block
 * (`muted`, with its icon). The `src/ui` Button has no text-size override, so the settings pages keep this one.
 */
export function SmallOutlineButton({ label, onPress, icon, muted = false, state = 'idle', accessibilityLabel }: { label: string; onPress: () => void; icon?: IconName; muted?: boolean; state?: 'idle' | 'copied' | 'failed'; accessibilityLabel?: string }) {
  const { colors, scheme } = useTheme()
  const flat = useButtonStyle() === 'flat'
  const edge = flat ? undefined : scheme === 'dark' ? alpha(cssColors.white, 0.08) : alpha(cssColors.black, 0.22)
  const tone = state === 'copied' ? 'text-primary' : state === 'failed' ? 'text-destructive' : muted ? 'text-muted-foreground' : 'text-foreground'
  const fill = state === 'copied' ? 'bg-primary/10' : state === 'failed' ? 'bg-destructive/10' : muted ? 'bg-background/90' : 'bg-background dark:bg-input/30'
  const iconColor = state === 'copied' ? colors.primary : state === 'failed' ? colors.destructive : muted ? colors.mutedForeground : colors.foreground
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
      onPress={onPress}
      className={`h-7 flex-row items-center justify-center gap-1 rounded-md border border-border dark:border-input active:bg-muted ${muted ? 'px-2' : 'px-2.5'} ${fill}`}
      style={({ pressed }) => ({
        boxShadow: edge && !pressed ? `0 2px 0 ${edge}` : undefined,
        transform: flat ? [{ scale: pressed ? 0.97 : 1 }] : [{ translateY: pressed && edge ? 2 : 0 }],
      })}
    >
      {icon ? <Icon name={icon} size={14} color={iconColor} /> : null}
      <Text className={`${muted ? 'font-noto-semibold' : 'font-noto-medium'} text-[11px] leading-[16px] ${tone}`}>{label}</Text>
    </Pressable>
  )
}
