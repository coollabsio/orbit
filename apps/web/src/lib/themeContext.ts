import { createContext, useContext } from 'react'

export type Theme = 'dark' | 'light'

/** Account settings → Appearance → Font. `noto-sans` is the default; `system` is the operating system's UI font. */
export const FONTS = ['noto-sans', 'geist', 'inter', 'ibm-plex-sans', 'system', 'literata', 'mono'] as const

/** Account settings → Appearance → Text size, smallest first. */
export const TEXT_SIZES = ['small', 'compact', 'default', 'large', 'extra-large'] as const

/** The Account settings → Appearance choices, saved in this browser. */
export interface Appearance {
  /** `system` follows the operating system. */
  theme: Theme | 'system'
  /** `3d` buttons and sidebar icons rest on a raised edge; `flat` has none. */
  style: '3d' | 'flat'
  /** `floating` frames the sidebar and the content as panels on a dotted backdrop (desktop only). */
  navStyle: 'default' | 'floating'
  /** The typeface of the whole interface (`[data-font]` on `<html>`). */
  font: (typeof FONTS)[number]
  /** The root font size: every rem size, text and spacing, follows it (`[data-text-size]` on `<html>`). */
  textSize: (typeof TEXT_SIZES)[number]
}

export const ThemeContext = createContext<{
  /** The theme on screen: the chosen one, or the system's. */
  theme: Theme
  appearance: Appearance
  setAppearance: (change: Partial<Appearance>) => void
  toggleTheme: () => void
}>({
  theme: 'dark',
  appearance: { theme: 'dark', style: '3d', navStyle: 'default', font: 'noto-sans', textSize: 'default' },
  setAppearance: () => {},
  toggleTheme: () => {},
})

export function useTheme() {
  return useContext(ThemeContext)
}
