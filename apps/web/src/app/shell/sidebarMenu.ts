import type { MenuSurface } from '@/components/ui/menuSurface'
import { useAppSidebarTheme } from '@/lib/customTheme'

/**
 * The menus opened from the app sidebar (workspace, account, the docs and chat lists): the sidebar's popover colour
 * and pink tint, and the custom theme. The panels are portalled out of the sidebar, so the theme goes on their wrapper.
 */
export function useSidebarMenuSurface(): MenuSurface {
  const theme = useAppSidebarTheme()
  return {
    className: 'rounded-[10px] bg-sidebar-popover dark:ring-sidebar-tint/22 dark:**:data-[slot$=-item]:focus:bg-sidebar-tint/10 dark:**:data-[slot$=-item]:data-highlighted:bg-sidebar-tint/10',
    positionerClassName: theme.scheme,
    positionerStyle: theme.style,
  }
}
