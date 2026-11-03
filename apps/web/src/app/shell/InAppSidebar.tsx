import { useContext, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { MenuSurfaceContext } from '@/components/ui/menuSurface'
import { useSidebarMenuSurface } from './sidebarMenu'
import { SidebarSlotContext } from './sidebarSlot'

/** Renders a page's list in the app sidebar when it has room, otherwise in place. There its menus look like the sidebar's. */
export function InAppSidebar({ children }: { children: ReactNode }) {
  const slot = useContext(SidebarSlotContext)
  const surface = useSidebarMenuSurface()
  return slot ? createPortal(<MenuSurfaceContext value={surface}>{children}</MenuSurfaceContext>, slot) : children
}

/**
 * A page list's header inside the app sidebar: a small label in the sidebar's section style ("Personal", "Manage"),
 * with small plain buttons (`variant="ghost" size="icon-xs"`). The Back row above names the area, so the list has no title of its own.
 */
export function SidebarListHeader({ label, children }: { label: string; children?: ReactNode }) {
  return (
    <div data-slot="sidebar-list-header" className="flex h-7 shrink-0 items-center gap-0.5 pr-3.5 pl-[18px] [&_button]:text-sidebar-nav [&_button:hover]:text-sidebar-nav-active [&_button_svg]:size-3.5">
      <h2 className="min-w-0 flex-1 truncate text-[11px] leading-4 font-medium text-muted-foreground select-none">
        {label}
      </h2>
      {children}
    </div>
  )
}
