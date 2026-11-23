import { type ReactNode, useEffect, useState } from 'react'
import { Pressable, View } from 'react-native'

import { CustomThemeSheet, CustomThemeSwatch } from '../../../components/settings/CustomThemeSheet'
import { ACCOUNT_NAV, SettingsCard, SettingsShell } from '../../../components/settings/Shell'
import { APP_SECTIONS, type AppSection, DEFAULT_APP_LOOKS, themeName } from '../../../data/custom-theme'
import {
  type ButtonStyle,
  alpha,
  cssColors,
  palettes,
  type Scheme,
  setAppLook,
  setButtonStyle,
  setCustomTheme,
  setFont,
  setSchemePreference,
  setTextSize,
  useButtonStyle,
  useCustomThemeChoice,
  useFont,
  useSchemePreference,
  useTextSize,
  useTheme,
} from '../../../theme'
import { loadFont } from '../../../theme/font-files'
import { FONTS, type FontChoice, fontFace, type TextSize } from '../../../theme/typography'
import { Icon, type IconName, Text } from '../../../ui'

const THEMES: readonly { value: Scheme | 'system'; label: string; description: string }[] = [
  { value: 'light', label: 'Light', description: 'Bright surfaces and dark text.' },
  { value: 'system', label: 'System', description: 'Follow your operating system.' },
  { value: 'dark', label: 'Dark', description: 'Dark surfaces and soft contrast.' },
]

const SECTION_LABELS: Record<AppSection, string> = { docs: 'Docs', chat: 'Chat', tasks: 'Tasks' }

const STYLES: readonly { value: ButtonStyle; label: string; description: string }[] = [
  { value: '3d', label: '3D', description: 'Buttons and sidebar icons sit on a raised edge that lifts on hover.' },
  { value: 'flat', label: 'Flat', description: 'No raised edges. Sidebar items are plain rows with a highlight for the current page.' },
]

const FONT_LABELS: Record<FontChoice, string> = {
  'noto-sans': 'Noto Sans',
  geist: 'Geist',
  inter: 'Inter',
  'ibm-plex-sans': 'IBM Plex Sans',
  system: 'System',
  literata: 'Literata',
  mono: 'Mono',
}

const TEXT_SIZE_OPTIONS: readonly { value: TextSize; label: string; sample: string }[] = [
  { value: 'small', label: 'Small', sample: 'text-[14px]' },
  { value: 'compact', label: 'Compact', sample: 'text-[15px]' },
  { value: 'default', label: 'Default', sample: 'text-[16px]' },
  { value: 'large', label: 'Large', sample: 'text-[18px]' },
  { value: 'extra-large', label: 'Extra large', sample: 'text-[20px]' },
]

/** The web's Profile → Appearance: how Orbit looks on this device. Navigation style (Floating) is desktop-only. */
export default function AppearanceScreen() {
  const schemePreference = useSchemePreference()
  const buttonStyle = useButtonStyle()
  const font = useFont()
  const textSize = useTextSize()
  const choice = useCustomThemeChoice()
  const appLooks = choice && 'apps' in choice ? choice.apps : null
  const customTheme = choice && !('apps' in choice) ? choice : null
  // the colour editor: for the whole app (Custom), or for one part of it (Per app)
  const [editing, setEditing] = useState<'all' | AppSection | null>(null)
  // each font tile draws its sample in its own font, so every font is loaded while this screen is open
  const [fontsLoaded, setFontsLoaded] = useState(false)
  useEffect(() => {
    void Promise.all(FONTS.map(loadFont))
      .then(() => setFontsLoaded(true))
      .catch(() => {})
  }, [])

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
              selected={!choice && schemePreference === option.value}
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
            label={customTheme ? `Custom · ${themeName(customTheme)}` : 'Custom'}
            description="Your own colors for the whole app. Pick a preset or your own."
            selected={Boolean(customTheme)}
            onPress={() => setEditing('all')}
          >
            <CustomThemeSwatch theme={customTheme} className="h-12 w-[70%]" />
          </AppearanceOption>
          <AppearanceOption
            label="Per app"
            description="Different colors for docs, chat and tasks. Starts as Notion, Slack and Linear."
            selected={Boolean(appLooks)}
            onPress={() => {
              if (!appLooks) setCustomTheme({ apps: DEFAULT_APP_LOOKS })
            }}
          >
            <View className="flex-row gap-1.5">
              {APP_SECTIONS.map((section) => (
                <CustomThemeSwatch key={section} theme={(appLooks ?? DEFAULT_APP_LOOKS)[section]} className="h-10 w-9" />
              ))}
            </View>
          </AppearanceOption>
        </View>
      </SettingsCard>

      {appLooks ? (
        <SettingsCard title="Per app colors" description="Pick the colors of each part of Orbit. Orbit uses your Light, System or Dark theme, as set above.">
          <View className="gap-3">
            {APP_SECTIONS.map((section) => (
              <AppearanceOption key={section} label={SECTION_LABELS[section]} description={themeName(appLooks[section])} onPress={() => setEditing(section)}>
                <CustomThemeSwatch theme={appLooks[section]} className="h-12 w-[70%]" />
              </AppearanceOption>
            ))}
          </View>
        </SettingsCard>
      ) : null}

      <SettingsCard title="Style" description="Choose how buttons and sidebar items look.">
        <View className="gap-3">
          {STYLES.map((option) => (
            <AppearanceOption key={option.value} label={option.label} description={option.description} selected={buttonStyle === option.value} onPress={() => setButtonStyle(option.value)}>
              <StylePreview tiles={option.value === '3d'} />
            </AppearanceOption>
          ))}
        </View>
      </SettingsCard>

      <SettingsCard title="Font" description="The typeface for the whole interface.">
        {/* the web's `grid grid-cols-2 gap-3` */}
        <View className="-m-1.5 flex-row flex-wrap">
          {FONTS.map((option) => (
            <View key={option} className="w-[50%] p-1.5">
              <AppearanceOption label={FONT_LABELS[option]} selected={font === option} onPress={() => void setFont(option)}>
                <Text className="text-[30px] leading-[36px] text-foreground" style={fontsLoaded ? fontFace(option, '400Regular') : undefined}>
                  Ag
                </Text>
              </AppearanceOption>
            </View>
          ))}
        </View>
      </SettingsCard>

      <SettingsCard title="Text size" description="Scales the whole interface, text and spacing together.">
        <View accessibilityRole="radiogroup" accessibilityLabel="Text size" className="flex-row gap-1 rounded-[10px] border border-border bg-card p-1">
          {TEXT_SIZE_OPTIONS.map((option) => {
            const selected = textSize === option.value
            return (
              <Pressable
                key={option.value}
                accessibilityRole="radio"
                accessibilityLabel={option.label}
                accessibilityState={{ selected }}
                onPress={() => setTextSize(option.value)}
                className={`min-w-0 flex-1 items-center gap-0.5 rounded-md px-1 py-2 ${selected ? 'bg-muted' : 'active:bg-muted/50'}`}
              >
                <Text aria-hidden className={`leading-none ${option.sample} ${selected ? 'text-foreground' : 'text-muted-foreground'}`}>
                  Aa
                </Text>
                <Text numberOfLines={1} className={`text-[11px] ${selected ? 'text-foreground' : 'text-muted-foreground'}`}>
                  {option.label}
                </Text>
              </Pressable>
            )
          })}
        </View>
      </SettingsCard>

      <CustomThemeSheet
        visible={editing === 'all'}
        title="Custom theme"
        description="Colours of the whole app, in place of Light, System or Dark. Only you see them, and text stays readable on every colour."
        value={customTheme}
        onSave={setCustomTheme}
        onClose={() => setEditing(null)}
      />
      {APP_SECTIONS.map((section) => (
        <CustomThemeSheet
          key={section}
          visible={editing === section}
          title={`${SECTION_LABELS[section]} colours`}
          description={`Colours of ${SECTION_LABELS[section].toLowerCase()} screens. Orbit uses your Light, System or Dark theme. Only you see them.`}
          value={appLooks?.[section] ?? null}
          onSave={(theme) => setAppLook(section, theme)}
          onClose={() => setEditing(null)}
        />
      ))}
    </SettingsShell>
  )
}

/** One choice: a picture of the look, its name, and a check when it is the current one. `selected: undefined` opens a dialog. */
function AppearanceOption({ label, description, selected, onPress, children }: { label: string; description?: string; selected?: boolean; onPress: () => void; children: ReactNode }) {
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
        {description ? <Text className="mt-1 text-[12px] leading-[20px] text-muted-foreground">{description}</Text> : null}
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
