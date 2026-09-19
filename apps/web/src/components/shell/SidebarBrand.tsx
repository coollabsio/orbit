import { WorkspaceSwitcher } from './WorkspaceSwitcher'

interface SidebarBrandProps {
  collapsed?: boolean
  onSelectWorkspace?: () => void
}

/** 48px brand row shared by the desktop sidebar and the mobile drawer. */
export function SidebarBrand({ collapsed = false, onSelectWorkspace }: SidebarBrandProps) {
  return (
    <div className="app-sidebar-brand">
      <WorkspaceSwitcher collapsed={collapsed} onSelect={onSelectWorkspace} />
    </div>
  )
}
