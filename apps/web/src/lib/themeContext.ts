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
  /**
   * `new` is the new UI: a contextual sidebar (settings links, the docs and chat lists, Back) and a custom theme that
   * colours the whole app. `classic` keeps the sidebar with the app links only, and a custom theme that colours chat.
   */
  layout: 'classic' | 'new'
}

export const ThemeContext = createContext<{
  /** The theme on screen: the chosen one, or the system's. */
  theme: Theme
  appearance: Appearance
  setAppearance: (change: Partial<Appearance>) => void
  toggleTheme: () => void
}>({
  theme: 'dark',
  appearance: { theme: 'dark', style: '3d', navStyle: 'default', layout: 'new' },
  setAppearance: () => {},
  toggleTheme: () => {},
})

export function useTheme() {
  return useContext(ThemeContext)
}

/** Account settings → Appearance → Layout is the new UI. */
export function useNewUi(): boolean {
  return useContext(ThemeContext).appearance.layout === 'new'
}
