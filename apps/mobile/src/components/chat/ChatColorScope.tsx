import { type ReactNode, useMemo } from 'react'

import { customSidebarTheme, customThemeColors, customThemeScheme } from '../../data/custom-theme'
import { useCustomTheme, useLayout } from '../../theme'
import { ColorScope } from '../../theme/ColorScope'

/**
 * The custom theme on chat only, in the classic layout (the web's `ChatPage`): the conversation list in the sidebar
 * colours, the other chat screens in the page colours. The new UI already has the theme on the whole app.
 */
export function ChatColorScope({ surface, children }: { surface: 'list' | 'page'; children: ReactNode }) {
  const theme = useCustomTheme()
  const classic = useLayout() === 'classic'
  const scope = useMemo(() => {
    if (!theme || !classic) return null
    return surface === 'list' ? customSidebarTheme(theme) : { scheme: customThemeScheme(theme), colors: customThemeColors(theme) }
  }, [theme, classic, surface])
  if (!scope) return children
  return (
    <ColorScope scheme={scope.scheme} colors={scope.colors}>
      {children}
    </ColorScope>
  )
}
