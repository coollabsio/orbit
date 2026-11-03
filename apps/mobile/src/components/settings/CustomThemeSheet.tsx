import { useState } from 'react'
import { Platform, Pressable, Share, useColorScheme, View } from 'react-native'

import { CUSTOM_THEME_PRESETS, type CustomTheme, normalizeHex, parseCustomTheme, readableOn, serializeCustomTheme } from '../../data/custom-theme'
import { palettes, type Scheme, setCustomTheme, useCustomTheme, useSchemePreference, useTheme } from '../../theme'
import { Button, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'
import { Dialog } from './Dialog'

/** The colours the fields start from when the user edits the Orbit default. */
const CUSTOM_BASE: CustomTheme = { sidebar: '#1f1f23', selected: '#34343b', accent: '#f2458f', chat: '#26262b' }

const COLOR_FIELDS: readonly { key: keyof CustomTheme; label: string; hint: string }[] = [
  { key: 'sidebar', label: 'Sidebar', hint: 'The app sidebar' },
  { key: 'selected', label: 'Selected', hint: 'The open row in a list' },
  { key: 'accent', label: 'Accent', hint: 'Unread, badges, mentions' },
  { key: 'chat', label: 'Pages', hint: 'Every page, menus, dialogs' },
]

/** The web's `CustomThemeDialog`: presets, four colours and a code to share. Saved on this device. */
export function CustomThemeSheet({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  return (
    <Dialog
      visible={visible}
      title="Custom theme"
      description="Colours of the whole app, in place of Light, System or Dark. Only you see them, and text stays readable on every colour."
      onClose={onClose}
      footer={null}
    >
      {/* mounted only while open, so every opening starts from the saved theme */}
      {visible ? <CustomThemeForm onDone={onClose} /> : null}
    </Dialog>
  )
}

function CustomThemeForm({ onDone }: { onDone: () => void }) {
  const { colors: palette } = useTheme()
  const saved = useCustomTheme()
  const [theme, setTheme] = useState<CustomTheme | null>(saved)
  // Changes when the colours come from a preset or a pasted code, so the hex fields take the new values.
  const [revision, setRevision] = useState(0)
  const [codeError, setCodeError] = useState(false)
  const colors = theme ?? CUSTOM_BASE
  const code = serializeCustomTheme(colors)

  const replace = (next: CustomTheme | null) => {
    setTheme(next)
    setRevision((current) => current + 1)
    setCodeError(false)
  }

  return (
    <View className="gap-5">
      <View className="gap-2">
        <Text className="font-noto-medium text-[12px] text-muted-foreground">Presets</Text>
        <View accessibilityRole="radiogroup" accessibilityLabel="Presets" className="flex-row flex-wrap gap-y-2.5 -mx-1">
          <PresetButton name="Orbit" theme={null} pressed={theme === null} onSelect={() => replace(null)} />
          {CUSTOM_THEME_PRESETS.map((preset) => (
            <PresetButton
              key={preset.name}
              name={preset.name}
              theme={preset.theme}
              pressed={theme !== null && serializeCustomTheme(theme) === serializeCustomTheme(preset.theme)}
              onSelect={() => replace(preset.theme)}
            />
          ))}
        </View>
      </View>

      <CustomThemeSwatch theme={theme} className="h-20" />

      <View className="gap-2">
        <Text className="font-noto-medium text-[12px] text-muted-foreground">Colours</Text>
        <View className="gap-1.5">
          {COLOR_FIELDS.map((field) => (
            <ColorField
              key={`${field.key}-${revision}`}
              label={field.label}
              hint={field.hint}
              value={colors[field.key]}
              onChange={(color) => {
                setTheme({ ...colors, [field.key]: color })
                setCodeError(false)
              }}
            />
          ))}
        </View>
      </View>

      <View className="gap-1.5">
        <Text className="font-noto-medium text-[12px] text-muted-foreground">Share code</Text>
        <View className="flex-row items-center gap-2">
          <TextInput
            key={revision}
            accessibilityLabel="Share code"
            defaultValue={code}
            autoCapitalize="none"
            autoCorrect={false}
            spellCheck={false}
            placeholderTextColor={palette.mutedForeground}
            onChangeText={(text) => {
              const parsed = parseCustomTheme(text)
              setCodeError(!parsed)
              if (parsed) setTheme(parsed)
            }}
            className={`flex-1 min-h-8 rounded-lg border px-2.5 py-[3px] font-mono text-[13px] text-foreground dark:bg-input/30 ${codeError ? 'border-destructive' : 'border-input focus:border-ring'}`}
          />
          <Button
            label={Platform.OS === 'web' ? 'Copy' : 'Share'}
            icon={Platform.OS === 'web' ? 'copy' : 'send'}
            variant="outline"
            size="sm"
            onPress={() => {
              if (Platform.OS === 'web') void navigator.clipboard.writeText(code)
              else void Share.share({ message: code })
            }}
          />
        </View>
        <Text className={`text-[12px] leading-[16px] ${codeError ? 'text-destructive' : 'text-muted-foreground'}`}>
          {codeError ? 'A code is four hex colours, for example #3f0e40,#1164a3,#ecb22e,#ffffff.' : 'Paste a code that someone shared, or share yours.'}
        </Text>
      </View>

      <View className="flex-row flex-wrap items-center gap-2">
        {saved ? (
          <Button
            label="Reset"
            variant="ghost"
            onPress={() => {
              setCustomTheme(null)
              onDone()
            }}
          />
        ) : null}
        <View className="flex-1" />
        <Button label="Cancel" variant="outline" onPress={onDone} />
        <Button
          label="Save theme"
          disabled={codeError}
          onPress={() => {
            setCustomTheme(theme)
            onDone()
          }}
        />
      </View>
    </View>
  )
}

/** A preset as a thumbnail of chat in its colours, with its name below. "Orbit" (`theme` null) is the default look. */
function PresetButton({ name, theme, pressed, onSelect }: { name: string; theme: CustomTheme | null; pressed: boolean; onSelect: () => void }) {
  return (
    <Pressable
      accessibilityRole="radio"
      accessibilityLabel={name}
      accessibilityState={{ selected: pressed }}
      onPress={onSelect}
      className="w-[33.333%] px-1 gap-1 active:opacity-80"
    >
      <CustomThemeSwatch theme={theme} className={`h-12 ${pressed ? 'border-primary shadow-[0_0_0_2px_var(--color-primary)]' : ''}`} />
      <Text numberOfLines={1} className={`px-0.5 text-[12px] ${pressed ? 'font-noto-medium text-foreground' : 'text-muted-foreground'}`}>
        {name}
      </Text>
    </Pressable>
  )
}

/** A thumbnail of chat in the theme's colours. `null` is the Orbit default (the app's own colours). */
export function CustomThemeSwatch({ theme, className = '' }: { theme: CustomTheme | null; className?: string }) {
  // "Orbit" shows Orbit's own colours in the chosen Light or Dark, not the custom theme that may colour the screen now
  const orbit = palettes[useOrbitScheme()]
  const colors = theme ?? { sidebar: orbit.sidebar, selected: orbit.sidebarAccent, accent: orbit.primary, chat: orbit.background }
  const sidebarText = theme ? readableOn(theme.sidebar) : orbit.sidebarForeground
  const chatText = theme ? readableOn(theme.chat) : orbit.foreground
  return (
    <View aria-hidden className={`flex-row overflow-hidden rounded-md border border-border ${className}`}>
      <View className="w-[38%] gap-[3px] p-1.5" style={{ backgroundColor: colors.sidebar }}>
        <View className="h-[3px] w-[75%] rounded-full opacity-35" style={{ backgroundColor: sidebarText }} />
        <View className="-mx-0.5 h-[7px] rounded-[2px]" style={{ backgroundColor: colors.selected }} />
        <View className="flex-row items-center justify-between">
          <View className="h-[3px] w-[50%] rounded-full opacity-35" style={{ backgroundColor: sidebarText }} />
          <View className="size-[4px]" style={{ backgroundColor: colors.accent }} />
        </View>
      </View>
      <View className="flex-1 gap-[3px] p-1.5" style={{ backgroundColor: colors.chat }}>
        <View className="h-[3px] w-[80%] rounded-full opacity-45" style={{ backgroundColor: chatText }} />
        <View className="h-[3px] w-[60%] rounded-full opacity-25" style={{ backgroundColor: chatText }} />
        <View className="mt-auto h-[7px] rounded-[2px] border opacity-25" style={{ borderColor: chatText }} />
      </View>
    </View>
  )
}

/** One colour: its swatch, what it is for, and a hex field. An invalid value keeps the last good colour. */
function ColorField({ label, hint, value, onChange }: { label: string; hint: string; value: string; onChange: (color: string) => void }) {
  const { colors } = useTheme()
  const [text, setText] = useState(value)
  const invalid = normalizeHex(text) === null
  return (
    <View className="flex-row items-center gap-2.5">
      <View className="size-8 rounded-md border border-input" style={{ backgroundColor: value }} />
      <View className="flex-1 min-w-0">
        <Text numberOfLines={1} className="text-[14px] leading-[18px]">
          {label}
        </Text>
        <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">
          {hint}
        </Text>
      </View>
      <TextInput
        accessibilityLabel={`${label} colour`}
        value={text}
        maxLength={7}
        autoCapitalize="none"
        autoCorrect={false}
        spellCheck={false}
        placeholderTextColor={colors.mutedForeground}
        onChangeText={(next) => {
          setText(next)
          const color = normalizeHex(next)
          if (color) onChange(color)
        }}
        onBlur={() => setText(value)}
        className={`w-[92px] min-h-8 rounded-lg border px-2.5 py-[3px] font-mono text-[13px] text-foreground dark:bg-input/30 ${invalid ? 'border-destructive' : 'border-input focus:border-ring'}`}
      />
    </View>
  )
}

/** Light or Dark as chosen in Settings (System follows the device), whatever custom theme is on. */
function useOrbitScheme(): Scheme {
  const preference = useSchemePreference()
  const system = useColorScheme()
  return preference === 'system' ? (system === 'dark' ? 'dark' : 'light') : preference
}
