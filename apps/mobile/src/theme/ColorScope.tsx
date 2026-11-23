import { type ReactElement, type ReactNode, useMemo } from 'react'
import { ScopedTheme, ScopedVariables } from 'uniwind'

import { type AppSection, appSectionOf, colorVariables, customThemeColors } from '../data/custom-theme'
import { mix } from '../data/color'
import { type Colors, ColorScopeContext, cssColors, palettes, type Scheme, useCustomThemeOf, useOpenSection, useSchemeOf } from './index'

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
 * The app's colours in one part of the app (`null`: outside Docs, Chat and Tasks): the scheme, the custom theme, and
 * the selected row in the sidebar's own colour (a bit darker, lighter in dark mode) like the web's `--sidebar-accent`.
 */
export function useAppPalette(part: AppSection | null): { scheme: Scheme; colors: Partial<Colors> } {
  const custom = useCustomThemeOf(part)
  const scheme = useSchemeOf(custom)
  return useMemo(() => {
    const sidebar = palettes[scheme].sidebar
    const sidebarAccent = scheme === 'dark' ? mix(sidebar, 0.91, cssColors.white) : mix(sidebar, 0.94, cssColors.black)
    return { scheme, colors: { sidebarAccent, ...(custom ? customThemeColors(custom) : {}) } }
  }, [scheme, custom])
}

/** Puts the app's colours of one part of the app (`useAppPalette`) on everything below. */
export function AppColorScope({ part = null, children }: { part?: AppSection | null; children: ReactNode }) {
  const { scheme, colors } = useAppPalette(part)
  return (
    <ColorScope scheme={scheme} colors={colors}>
      {children}
    </ColorScope>
  )
}

/** The colours of the part of the app that is open, for what all parts share: the top bar and the dock. */
export function OpenColorScope({ children }: { children: ReactNode }) {
  return <AppColorScope part={useOpenSection()}>{children}</AppColorScope>
}

/**
 * A navigator's `screenLayout`: Per app gives Docs, Chat and Tasks their own colours that do not change with the open
 * page, so a move to another part of the app does not render the hidden screens again.
 */
export function partScreenLayout({ route, children }: { route: { name: string }; children: ReactElement }) {
  const part = appSectionOf(`/${route.name}`)
  return part ? <AppColorScope part={part}>{children}</AppColorScope> : children
}
