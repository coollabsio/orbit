import { SidebarLeft } from 'reicon-react'
import { SIDEBAR_TOGGLE_KEY } from './sidebarShortcut'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

interface SidebarBrandProps {
  collapsed?: boolean
  /** Omitted by the mobile drawer, which has no collapsed state. */
  onToggleCollapse?: () => void
  onSelectWorkspace?: () => void
}

/** 48px brand row shared by the desktop sidebar and the mobile drawer. */
export function SidebarBrand({ collapsed = false, onToggleCollapse, onSelectWorkspace }: SidebarBrandProps) {
  const label = collapsed ? 'Expand sidebar' : 'Collapse sidebar'
  return (
    <div className="app-sidebar-brand">
      <WorkspaceSwitcher collapsed={collapsed} onSelect={onSelectWorkspace} />
      {onToggleCollapse ? (
        <button
          type="button"
          className="icon-button app-sidebar-collapse"
          aria-label={label}
          title={`${label} (${SIDEBAR_TOGGLE_KEY})`}
          onClick={onToggleCollapse}
        >
          <SidebarLeft size={17} />
        </button>
      ) : null}
    </div>
  )
}
