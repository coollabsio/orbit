import { createContext, use, useSyncExternalStore } from 'react'
import { Platform, useColorScheme } from 'react-native'
import { Uniwind } from 'uniwind'

import { alpha, mix } from '../data/color'
import {
  activeCustomTheme,
  type AppSection,
  appSectionOf,
  type CustomTheme,
  type CustomThemeChoice,
  customThemeScheme,
  DEFAULT_APP_LOOKS,
  parseCustomThemeChoice,
  serializeCustomThemeChoice,
} from '../data/custom-theme'
import { storage } from '../storage'
import { loadFont } from './font-files'
import { dark, light } from './tokens.gen'
import { FONTS, type FontChoice, TEXT_SCALE, TEXT_SIZES, type TextSize } from './typography'

export type ColorName = keyof typeof light
export type Colors = Record<ColorName, string>
export type Scheme = 'light' | 'dark'

/** Noto Sans faces loaded in the root layout. Each weight is its own family, so set `fontFamily`, not `fontWeight`. */
export const fonts = {
  regular: 'NotoSans_400Regular',
  medium: 'NotoSans_500Medium',
  semibold: 'NotoSans_600SemiBold',
  bold: 'NotoSans_700Bold',
  mono: Platform.select({ ios: 'Menlo', default: 'monospace' }),
} as const

export const palettes: Record<Scheme, Colors> = { light, dark }

/** The CSS keywords the web mixes with: `color-mix(…, black 25%)` button edges, the `bg-black/50` scrim. */
export const cssColors = { black: '#000000', white: '#ffffff' } as const

/** Tailwind green-500, amber-500 and red-500: the web `StatusMark` draws presence with these in both themes. */
export const presenceColors = { online: '#00c950', idle: '#fe9a00', dnd: '#fb2c36' } as const
/** A starred conversation's star (the web's `fill-[#fcd452]` in the mail list and thread). */
export const starColor = '#fcd452'

export { alpha, mix }

export interface Theme {
  scheme: Scheme
  colors: Colors
  fonts: typeof fonts
}

const SCHEME_KEY = 'orbit.theme'
const STYLE_KEY = 'orbit.style'
const CUSTOM_THEME_KEY = 'orbit.customTheme'
const FONT_KEY = 'orbit.font'
const TEXT_SIZE_KEY = 'orbit.textSize'
/** `null` follows the system; the Profile header's theme button sets one explicitly, like the web toggle. */
let preference: Scheme | null = null
/** The web's Appearance → Style: `3d` buttons rest on a raised edge, `flat` ones have none. */
export type ButtonStyle = '3d' | 'flat'
let buttonStyle: ButtonStyle = '3d'
/** Settings → Appearance → Custom or Per app. `null` is the chosen Light, System or Dark. */
let customTheme: CustomThemeChoice = null
/** The part of the app that is open, for Per app (`setOpenPage`). */
let section: AppSection | null = null
let font: FontChoice = 'noto-sans'
let textSize: TextSize = 'default'
const listeners = new Set<() => void>()

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function setSchemePreference(scheme: Scheme | null) {
  preference = scheme
  // Uniwind's `dark:` classes follow the same choice ('system' follows the device again).
  Uniwind.setTheme(scheme ?? 'system')
  listeners.forEach((listener) => listener())
  void (scheme ? storage.set(SCHEME_KEY, scheme) : storage.remove(SCHEME_KEY))
}

export function setButtonStyle(style: ButtonStyle) {
  buttonStyle = style
  listeners.forEach((listener) => listener())
  void storage.set(STYLE_KEY, style)
}

/** Saves the custom theme on this device. `null` goes back to Light, System or Dark. */
export function setCustomTheme(theme: CustomThemeChoice) {
  customTheme = theme
  listeners.forEach((listener) => listener())
  void (theme ? storage.set(CUSTOM_THEME_KEY, serializeCustomThemeChoice(theme)) : storage.remove(CUSTOM_THEME_KEY))
}

/** Per app: new colours for one part of the app, and Per app on when it was not. */
export function setAppLook(part: AppSection, theme: CustomTheme | null) {
  const looks = customTheme && 'apps' in customTheme ? customTheme.apps : DEFAULT_APP_LOOKS
  setCustomTheme({ apps: { ...looks, [part]: theme } })
}

/** The root layout tells which screen is open, so that Per app can colour it. */
export function setOpenPage(pathname: string) {
  const next = appSectionOf(pathname)
  if (next === section) return
  section = next
  listeners.forEach((listener) => listener())
}

/** Loads the font's files first, so that no text shows in a font that is not there yet. */
export async function setFont(choice: FontChoice) {
  await loadFont(choice)
  font = choice
  listeners.forEach((listener) => listener())
  void (choice === 'noto-sans' ? storage.remove(FONT_KEY) : storage.set(FONT_KEY, choice))
}

/** Scales text, spacing and icons together, as the web's root font size does: Tailwind spacing is `--spacing` steps. */
export function setTextSize(size: TextSize) {
  textSize = size
  const step = 4 * TEXT_SCALE[size]
  for (const theme of Uniwind.themes) Uniwind.updateCSSVariables(theme, { '--spacing': Platform.OS === 'web' ? `${step}px` : step })
  listeners.forEach((listener) => listener())
  void (size === 'default' ? storage.remove(TEXT_SIZE_KEY) : storage.set(TEXT_SIZE_KEY, size))
}

/** Reads the stored choices once at startup. */
export async function loadAppearance(): Promise<void> {
  const read = (key: string) => storage.get(key).catch(() => null)
  const [scheme, style, storedTheme, storedFont, storedSize] = await Promise.all([read(SCHEME_KEY), read(STYLE_KEY), read(CUSTOM_THEME_KEY), read(FONT_KEY), read(TEXT_SIZE_KEY)])
  if (scheme === 'light' || scheme === 'dark') setSchemePreference(scheme)
  if (style === 'flat') setButtonStyle(style)
  const theme = storedTheme ? parseCustomThemeChoice(storedTheme) : null
  if (theme) setCustomTheme(theme)
  const storedChoice = FONTS.find((choice) => choice === storedFont)
  // a font that does not load leaves Noto Sans
  if (storedChoice) await setFont(storedChoice).catch(() => {})
  const size = TEXT_SIZES.find((choice) => choice === storedSize)
  if (size) setTextSize(size)
}

/** The saved choice, for Appearance: it shows which card is on, whatever screen is open. */
export function useCustomThemeChoice(): CustomThemeChoice {
  return useSyncExternalStore(subscribe, () => customTheme)
}

/** The colours on screen now: the custom theme, or under Per app the open part's. `null` is Light, System or Dark. */
export function useCustomTheme(): CustomTheme | null {
  return useSyncExternalStore(subscribe, () => activeCustomTheme(customTheme, section))
}

export function useFont(): FontChoice {
  return useSyncExternalStore(subscribe, () => font)
}

export function useTextSize(): TextSize {
  return useSyncExternalStore(subscribe, () => textSize)
}

/** Settings → Appearance → Text size as a factor: text, spacing, icons and avatars grow or shrink by it. */
export function useUiScale(): number {
  return TEXT_SCALE[useTextSize()]
}

/** The font and the text scale that `ui/scaled` puts on every text. */
export function useTypography(): { font: FontChoice; scale: number } {
  return { font: useFont(), scale: useUiScale() }
}

/** The colours of a part of the screen (`ColorScope`): the whole app, the drawer, or chat. */
export const ColorScopeContext = createContext<{ scheme: Scheme; colors: Colors } | null>(null)

/** The choice in Settings → Appearance: `system` when none is set. */
export function useSchemePreference(): Scheme | 'system' {
  return useSyncExternalStore(subscribe, () => preference ?? 'system')
}

export function useButtonStyle(): ButtonStyle {
  return useSyncExternalStore(subscribe, () => buttonStyle)
}

/** The app's scheme: a custom theme decides it from its message colour, otherwise the chosen one. */
export function useAppScheme(): Scheme {
  const system = useColorScheme()
  const chosen = useSyncExternalStore(subscribe, () => preference)
  const custom = useCustomTheme()
  if (custom) return customThemeScheme(custom)
  return chosen ?? (system === 'dark' ? 'dark' : 'light')
}

export function useTheme(): Theme {
  const scope = use(ColorScopeContext)
  const scheme = useAppScheme()
  return scope ? { ...scope, fonts } : { scheme, colors: palettes[scheme], fonts }
}
