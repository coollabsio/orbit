import { createContext, useContext } from 'react'

export type Theme = 'dark' | 'light'

/** The Account settings → Appearance choices, saved in this browser. */
export interface Appearance {
  /** `system` follows the operating system. */
  theme: Theme | 'system'
  /** `3d` buttons and sidebar icons rest on a raised edge; `flat` has none. */
  style: '3d' | 'flat'
  /** `floating` frames the sidebar and the content as panels on a dotted backdrop (desktop only). */
  navStyle: 'default' | 'floating'
}

export const ThemeContext = createContext<{
  /** The theme on screen: the chosen one, or the system's. */
  theme: Theme
  appearance: Appearance
  setAppearance: (change: Partial<Appearance>) => void
  toggleTheme: () => void
}>({
  theme: 'dark',
  appearance: { theme: 'dark', style: '3d', navStyle: 'default' },
  setAppearance: () => {},
  toggleTheme: () => {},
})

export function useTheme() {
  return useContext(ThemeContext)
}
