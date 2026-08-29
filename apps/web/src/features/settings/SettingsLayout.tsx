import { NavLink, Outlet } from 'react-router'
import { People, Setting2, Trash } from 'reicon-react'
import { WebhookIcon } from '../../components/ui/WebhookIcon'
import '../shared/cards.css'
import './settings.css'
import './server.css'

interface NavItem {
  to: string
  label: string
  icon: React.ComponentType<{ size?: number }>
  end?: boolean
  danger?: boolean
}

const SECTIONS: { label: string; items: NavItem[] }[] = [
  {
    label: 'Server settings',
    items: [
      { to: '/settings', label: 'General', icon: Setting2, end: true },
      { to: '/settings/roles', label: 'Roles', icon: People },
      { to: '/settings/webhooks', label: 'Webhooks', icon: WebhookIcon },
      { to: '/settings/danger', label: 'Danger Zone', icon: Trash, danger: true },
    ],
  },
  {
    label: 'Team',
    items: [{ to: '/settings/members', label: 'Members', icon: People }],
  },
]

/** Coolify `x-settings.layout`: sticky sub-navigation (210px) + content column. */
export function SettingsLayout() {
  return (
    <div className="page">
      <div className="pane" style={{ flex: 1 }}>
        <div className="pane-header">
          <span className="pane-title">Settings</span>
        </div>
        <div className="settings-scroll">
          <section className="settings-workspace">
            <aside className="settings-nav">
              <nav aria-label="Settings">
                {SECTIONS.map((section) => (
                  <div key={section.label} style={{ display: 'contents' }}>
                    <div className="nav-section">{section.label}</div>
                    {section.items.map((item) => (
                      <NavLink
                        key={item.to}
                        to={item.to}
                        end={item.end}
                        className={({ isActive }) => (isActive ? 'menu-item active' : 'menu-item')}
                        data-danger={item.danger ? 'true' : undefined}
                      >
                        <item.icon size={18} />
                        <span className="menu-item-label">{item.label}</span>
                      </NavLink>
                    ))}
                  </div>
                ))}
              </nav>
            </aside>
            <div className="settings-content">
              <Outlet />
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}
