import { Link } from 'react-router'
import { Menu } from 'reicon-react'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { useTopbarSlotTarget } from './TopbarSlot'

/**
 * The single global 48px topbar. It owns only the workspace root crumb and the mobile drawer
 * button; every page fills the two slots from its own tree, so no route data is loaded here.
 */
export function Topbar({ onOpenDrawer }: { onOpenDrawer: () => void }) {
  const { workspace } = useWorkspace()
  const leftSlot = useTopbarSlotTarget('left')
  const rightSlot = useTopbarSlotTarget('right')

  return (
    <header className="topbar">
      <button className="icon-button topbar-menu-button topbar-drawer-button" onClick={onOpenDrawer} aria-label="Menu">
        <Menu size={18} />
      </button>
      <nav className="topbar-crumbs" aria-label="Breadcrumb">
        <Link className="topbar-crumb" to="/">{workspace.name}</Link>
        <div className="topbar-slot" data-slot="left" ref={leftSlot} />
      </nav>
      <div className="topbar-slot" data-slot="right" ref={rightSlot} />
    </header>
  )
}
