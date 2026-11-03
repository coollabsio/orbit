import { createContext, use, useSyncExternalStore } from 'react'
import { Platform, useColorScheme } from 'react-native'
import { Uniwind } from 'uniwind'

import { alpha, mix } from '../data/color'
import { customThemeScheme, parseCustomTheme, serializeCustomTheme, type CustomTheme } from '../data/custom-theme'
import { storage } from '../storage'
import { dark, light } from './tokens.gen'

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

export { alpha, mix }

export interface Theme {
  scheme: Scheme
  colors: Colors
  fonts: typeof fonts
}

const SCHEME_KEY = 'orbit.theme'
const STYLE_KEY = 'orbit.style'
const LAYOUT_KEY = 'orbit.layout'
const CUSTOM_THEME_KEY = 'orbit.customTheme'
/** `null` follows the system; the Profile header's theme button sets one explicitly, like the web toggle. */
let preference: Scheme | null = null
/** The web's Appearance → Style: `3d` buttons rest on a raised edge, `flat` ones have none. */
export type ButtonStyle = '3d' | 'flat'
let buttonStyle: ButtonStyle = '3d'
/**
 * The web's Appearance → Layout. `new` is the new UI: a contextual drawer (settings links and Back) and a custom theme
 * that colours the whole app. `classic` keeps the drawer with the app links only, and a custom theme that colours chat.
 */
export type Layout = 'classic' | 'new'
let layout: Layout = 'new'
/** Settings → Appearance → Custom. `null` is the chosen Light, System or Dark. */
let customTheme: CustomTheme | null = null
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

export function setLayout(next: Layout) {
  layout = next
  listeners.forEach((listener) => listener())
  void storage.set(LAYOUT_KEY, next)
}

/** Saves the custom theme on this device. `null` goes back to Light, System or Dark. */
export function setCustomTheme(theme: CustomTheme | null) {
  customTheme = theme
  listeners.forEach((listener) => listener())
  void (theme ? storage.set(CUSTOM_THEME_KEY, serializeCustomTheme(theme)) : storage.remove(CUSTOM_THEME_KEY))
}

/** Reads the stored choices once at startup. */
export async function loadAppearance(): Promise<void> {
  const read = (key: string) => storage.get(key).catch(() => null)
  const [scheme, style, storedLayout, storedTheme] = await Promise.all([read(SCHEME_KEY), read(STYLE_KEY), read(LAYOUT_KEY), read(CUSTOM_THEME_KEY)])
  if (scheme === 'light' || scheme === 'dark') setSchemePreference(scheme)
  if (style === 'flat') setButtonStyle(style)
  if (storedLayout === 'classic') setLayout(storedLayout)
  const theme = storedTheme ? parseCustomTheme(storedTheme) : null
  if (theme) setCustomTheme(theme)
}

export function useLayout(): Layout {
  return useSyncExternalStore(subscribe, () => layout)
}

/** The stored custom theme, whatever the layout. */
export function useCustomTheme(): CustomTheme | null {
  return useSyncExternalStore(subscribe, () => customTheme)
}

/** The custom theme that colours the whole app: only in the new UI. */
export function useAppCustomTheme(): CustomTheme | null {
  const theme = useCustomTheme()
  return useLayout() === 'new' ? theme : null
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

/** The app's scheme: a custom theme in the new UI decides it from its message colour, otherwise the chosen one. */
export function useAppScheme(): Scheme {
  const system = useColorScheme()
  const chosen = useSyncExternalStore(subscribe, () => preference)
  const custom = useAppCustomTheme()
  if (custom) return customThemeScheme(custom)
  return chosen ?? (system === 'dark' ? 'dark' : 'light')
}

export function useTheme(): Theme {
  const scope = use(ColorScopeContext)
  const scheme = useAppScheme()
  return scope ? { ...scope, fonts } : { scheme, colors: palettes[scheme], fonts }
}
