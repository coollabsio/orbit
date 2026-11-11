import { type ReactNode, useMemo } from 'react'
import { ScopedTheme, ScopedVariables } from 'uniwind'

import { colorVariables, customThemeColors } from '../data/custom-theme'
import { mix } from '../data/color'
import { type Colors, ColorScopeContext, cssColors, palettes, type Scheme, useAppScheme, useCustomTheme } from './index'

/**
 * Colours a part of the screen: Uniwind's classes (`bg-background`, `dark:`) and `useTheme().colors` below it. The web
 * sets CSS variables on an element for the same thing (the custom theme on the page, the sidebar, chat).
 */
export function ColorScope({ scheme, colors, children }: { scheme: Scheme; colors: Partial<Colors>; children: ReactNode }) {
  const value = useMemo(() => ({ scheme, colors: { ...palettes[scheme], ...colors } }), [scheme, colors])
  const variables = useMemo(() => colorVariables(colors), [colors])
  return (
    <ColorScopeContext value={value}>
      <ScopedTheme theme={scheme}>
        <ScopedVariables variables={variables}>{children}</ScopedVariables>
      </ScopedTheme>
    </ColorScopeContext>
  )
}

/**
 * The app's colours: the chosen scheme, the custom theme, and the selected row in the sidebar's own colour (a bit
 * darker, lighter in dark mode) like the web's `--sidebar-accent`.
 */
export function useAppPalette(): { scheme: Scheme; colors: Partial<Colors> } {
  const scheme = useAppScheme()
  const custom = useCustomTheme()
  return useMemo(() => {
    const sidebar = palettes[scheme].sidebar
    const sidebarAccent = scheme === 'dark' ? mix(sidebar, 0.91, cssColors.white) : mix(sidebar, 0.94, cssColors.black)
    return { scheme, colors: { sidebarAccent, ...(custom ? customThemeColors(custom) : {}) } }
  }, [scheme, custom])
}

/** Puts the app's colours (`useAppPalette`) on everything below. Mount once, at the root. */
export function AppColorScope({ children }: { children: ReactNode }) {
  const { scheme, colors } = useAppPalette()
  return (
    <ColorScope scheme={scheme} colors={colors}>
      {children}
    </ColorScope>
  )
}
