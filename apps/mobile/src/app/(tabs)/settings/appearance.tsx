import { type ReactNode, useState } from 'react'
import { Pressable, View } from 'react-native'

import { CustomThemeSheet, CustomThemeSwatch } from '../../../components/settings/CustomThemeSheet'
import { ACCOUNT_NAV, SettingsCard, SettingsShell } from '../../../components/settings/Shell'
import { CUSTOM_THEME_PRESETS, serializeCustomTheme } from '../../../data/custom-theme'
import {
  type ButtonStyle,
  alpha,
  cssColors,
  palettes,
  type Scheme,
  setButtonStyle,
  setCustomTheme,
  setSchemePreference,
  useButtonStyle,
  useCustomTheme,
  useSchemePreference,
  useTheme,
} from '../../../theme'
import { Icon, type IconName, Text } from '../../../ui'

const THEMES: readonly { value: Scheme | 'system'; label: string; description: string }[] = [
  { value: 'light', label: 'Light', description: 'Bright surfaces and dark text.' },
  { value: 'system', label: 'System', description: 'Follow your operating system.' },
  { value: 'dark', label: 'Dark', description: 'Dark surfaces and soft contrast.' },
]

const STYLES: readonly { value: ButtonStyle; label: string; description: string }[] = [
  { value: '3d', label: '3D', description: 'Buttons and sidebar icons sit on a raised edge that lifts on hover.' },
  { value: 'flat', label: 'Flat', description: 'No raised edges. Sidebar items are plain rows with a highlight for the current page.' },
]

/** The web's Profile → Appearance: how Orbit looks on this device. Navigation style (Floating) is desktop-only. */
export default function AppearanceScreen() {
  const schemePreference = useSchemePreference()
  const buttonStyle = useButtonStyle()
  const customTheme = useCustomTheme()
  const [customThemeOpen, setCustomThemeOpen] = useState(false)
  const customThemeName = customTheme
    ? (CUSTOM_THEME_PRESETS.find((preset) => serializeCustomTheme(preset.theme) === serializeCustomTheme(customTheme))?.name ?? 'Your colors')
    : null

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV}>
      <SettingsCard title="Color theme" description="Choose the color scheme used on this device.">
        {/* the web's `grid gap-3`: one column at phone width */}
        <View className="gap-3">
          {THEMES.map((option) => (
            <AppearanceOption
              key={option.value}
              label={option.label}
              description={option.description}
              selected={!customTheme && schemePreference === option.value}
              onPress={() => {
                setCustomTheme(null)
                setSchemePreference(option.value === 'system' ? null : option.value)
              }}
            >
              <SchemePreview scheme={option.value} />
            </AppearanceOption>
          ))}
          {/* the custom theme opens its editor: it is a theme of its own, and picking one above turns it off */}
          <AppearanceOption
            label={customTheme ? `Custom · ${customThemeName}` : 'Custom'}
            description="Your own colors for the whole app. Pick a preset or your own."
            selected={Boolean(customTheme)}
            onPress={() => setCustomThemeOpen(true)}
          >
            <CustomThemeSwatch theme={customTheme} className="h-12 w-[70%]" />
          </AppearanceOption>
        </View>
      </SettingsCard>

      <SettingsCard title="Style" description="Choose how buttons and sidebar items look.">
        <View className="gap-3">
          {STYLES.map((option) => (
            <AppearanceOption key={option.value} label={option.label} description={option.description} selected={buttonStyle === option.value} onPress={() => setButtonStyle(option.value)}>
              <StylePreview tiles={option.value === '3d'} />
            </AppearanceOption>
          ))}
        </View>
      </SettingsCard>

      <CustomThemeSheet visible={customThemeOpen} onClose={() => setCustomThemeOpen(false)} />
    </SettingsShell>
  )
}

/** One choice: a picture of the look, its name, and a check when it is the current one. `selected: undefined` opens a dialog. */
function AppearanceOption({ label, description, selected, onPress, children }: { label: string; description: string; selected?: boolean; onPress: () => void; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <Pressable
      accessibilityRole={selected === undefined ? 'button' : 'radio'}
      accessibilityLabel={label}
      accessibilityState={selected === undefined ? undefined : { selected }}
      onPress={onPress}
      className={`overflow-hidden rounded-[10px] border ${selected ? 'border-primary bg-primary/5' : 'border-border bg-card active:border-foreground/20'}`}
    >
      <View aria-hidden className="h-20 flex-row items-center justify-center overflow-hidden border-b border-border bg-muted/40">
        {children}
      </View>
      <View className="p-3">
        <View className="flex-row items-center justify-between gap-2">
          <Text numberOfLines={1} className="font-noto-semibold text-[14px] shrink">
            {label}
          </Text>
          {selected ? <Icon name="checkCircle" size={16} color={colors.primary} /> : null}
        </View>
        <Text className="mt-1 text-[12px] leading-[20px] text-muted-foreground">{description}</Text>
      </View>
    </Pressable>
  )
}

/** A bar of placeholder text. */
function Bar({ className = '' }: { className?: string }) {
  const { colors } = useTheme()
  return <View className={`h-1.5 rounded-full ${className}`} style={{ backgroundColor: alpha(colors.mutedForeground, 0.3) }} />
}

/** An icon on a raised tile (3D, the web's sidebar tile at small size) or on its own (Flat). */
function IconTile({ icon, tile = true, raised = false }: { icon: IconName; tile?: boolean; raised?: boolean }) {
  const { colors, scheme } = useTheme()
  if (!tile) return <Icon name={icon} size={12} color={colors.mutedForeground} />
  return (
    <View
      className="size-5 items-center justify-center rounded-md border border-border bg-background"
      style={raised ? { boxShadow: `0 2px 0 ${alpha(scheme === 'dark' ? cssColors.white : cssColors.black, scheme === 'dark' ? 0.12 : 0.22)}` } : undefined}
    >
      <Icon name={icon} size={12} color={colors.mutedForeground} />
    </View>
  )
}

/** The scheme's own page colour (System shows both halves), with a small card on it. */
function SchemePreview({ scheme }: { scheme: Scheme | 'system' }) {
  return (
    <View className="flex-1 self-stretch flex-row items-center justify-center">
      <View className="absolute inset-0 flex-row">
        {(scheme === 'system' ? (['light', 'dark'] as const) : [scheme]).map((half) => (
          <View key={half} className="flex-1" style={{ backgroundColor: palettes[half].background }} />
        ))}
      </View>
      <View className="h-8 w-20 rounded-md border" style={{ backgroundColor: palettes[scheme === 'light' ? 'light' : 'dark'].muted, borderColor: palettes[scheme === 'light' ? 'light' : 'dark'].border }} />
    </View>
  )
}

/** Two sidebar rows: raised icon tiles (3D) or plain rows with the current one highlighted (Flat). */
function StylePreview({ tiles }: { tiles: boolean }) {
  const { colors } = useTheme()
  return (
    <View className="flex-1 self-stretch justify-center gap-1.5 px-4">
      {(['user', 'settings'] as const).map((icon, index) => (
        <View
          key={icon}
          className={`h-7 w-[60%] flex-row items-center gap-2 rounded-md ${tiles ? '' : 'px-1.5'}`}
          style={!tiles && index === 0 ? { backgroundColor: alpha(colors.foreground, 0.08) } : undefined}
        >
          <IconTile icon={icon} tile={tiles} raised={tiles} />
          <Bar className="w-16" />
        </View>
      ))}
    </View>
  )
}
