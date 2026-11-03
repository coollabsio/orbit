import { type ReactNode, useState } from 'react'
import { Pressable, View } from 'react-native'

import { CustomThemeSheet, CustomThemeSwatch } from '../../../components/settings/CustomThemeSheet'
import { ACCOUNT_NAV, SettingsCard, SettingsShell } from '../../../components/settings/Shell'
import { CUSTOM_THEME_PRESETS, serializeCustomTheme } from '../../../data/custom-theme'
import {
  type ButtonStyle,
  type Layout,
  alpha,
  cssColors,
  palettes,
  type Scheme,
  setButtonStyle,
  setCustomTheme,
  setLayout,
  setSchemePreference,
  useButtonStyle,
  useCustomTheme,
  useLayout,
  useSchemePreference,
  useTheme,
} from '../../../theme'
import { Icon, type IconName, Text } from '../../../ui'

const LAYOUTS: readonly { value: Layout; label: string; description: string }[] = [
  { value: 'classic', label: 'Classic', description: 'The sidebar always shows the app links. Settings, Docs and Chat have their own list beside the content. The custom theme colors chat.' },
  { value: 'new', label: 'New UI', description: 'The sidebar changes with the page: settings links, the page tree and conversations, with Back. The custom theme colors the whole app.' },
]

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
  const layout = useLayout()
  const schemePreference = useSchemePreference()
  const buttonStyle = useButtonStyle()
  const customTheme = useCustomTheme()
  // in the new UI the custom theme replaces the colour theme; in the classic layout it colours chat on top of it
  const newUi = layout === 'new'
  const [customThemeOpen, setCustomThemeOpen] = useState(false)
  const customThemeName = customTheme
    ? (CUSTOM_THEME_PRESETS.find((preset) => serializeCustomTheme(preset.theme) === serializeCustomTheme(customTheme))?.name ?? 'Your colors')
    : null

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV}>
      <SettingsCard title="Layout" description="Choose the classic layout or the new UI.">
        <View className="gap-3">
          {LAYOUTS.map((option) => (
            <AppearanceOption key={option.value} label={option.label} description={option.description} selected={layout === option.value} onPress={() => setLayout(option.value)}>
              <LayoutPreview contextual={option.value === 'new'} />
            </AppearanceOption>
          ))}
        </View>
      </SettingsCard>

      <SettingsCard title="Color theme" description="Choose the color scheme used on this device.">
        <View className="flex-row flex-wrap -m-1.5">
          {THEMES.map((option) => (
            <View key={option.value} className="w-[50%] p-1.5">
              <AppearanceOption
                label={option.label}
                description={option.description}
                selected={!(newUi && customTheme) && schemePreference === option.value}
                onPress={() => {
                  if (newUi) setCustomTheme(null)
                  setSchemePreference(option.value === 'system' ? null : option.value)
                }}
              >
                <SchemePreview scheme={option.value} />
              </AppearanceOption>
            </View>
          ))}
          {/* the custom theme opens its editor. New UI: it is a theme of its own, and picking one above turns it off.
              Classic: it colours chat on top of the theme above, so it is never the chosen one */}
          <View className="w-[50%] p-1.5">
            <AppearanceOption
              label={customTheme ? `Custom · ${customThemeName}` : 'Custom'}
              description={newUi ? 'Your own colors for the whole app. Pick a preset or your own.' : 'Your own chat colors. Pick a preset or your own.'}
              selected={newUi ? Boolean(customTheme) : undefined}
              onPress={() => setCustomThemeOpen(true)}
            >
              <CustomThemeSwatch theme={customTheme} className="h-12 w-[70%]" />
            </AppearanceOption>
          </View>
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

/** A bar of placeholder text; the first one of a list is the open row. */
function Bar({ active = false, className = '' }: { active?: boolean; className?: string }) {
  const { colors } = useTheme()
  return <View className={`h-1.5 rounded-full ${className}`} style={{ backgroundColor: alpha(active ? colors.primary : colors.mutedForeground, active ? 0.5 : 0.3) }} />
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

/** The sidebar: app icons, or Back and the page's list; classic adds a second list column. */
function LayoutPreview({ contextual }: { contextual: boolean }) {
  const { colors } = useTheme()
  return (
    <View className="flex-1 self-stretch flex-row">
      <View className="w-14 gap-1.5 border-r border-border bg-background px-2 py-2">
        {contextual ? (
          <>
            <View className="h-3 flex-row items-center gap-1">
              <Icon name="chevronLeft" size={10} color={colors.mutedForeground} />
              <Bar className="h-1 flex-1" />
            </View>
            {[0, 1, 2].map((index) => <Bar key={index} active={index === 0} />)}
          </>
        ) : (
          (['user', 'settings'] as const).map((icon) => <IconTile key={icon} icon={icon} raised />)
        )}
      </View>
      {contextual ? null : (
        <View className="w-12 gap-1.5 border-r border-border px-2 py-2">
          {[0, 1, 2].map((index) => <Bar key={index} active={index === 0} />)}
        </View>
      )}
      <View className="flex-1 bg-background" />
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
