import { useAppState } from '../../mock/store'
import { useTheme } from '../../lib/themeContext'
import { cx } from '../../lib/cx'
import { Avatar } from '../../components/ui/Avatar'
import type { User } from '../../mock/types'
import '../shared/cards.css'
import './settings.css'

const FEATURES = ['Tasks', 'Docs', 'Mail', 'Chat']

function SettingsCard({
  title,
  flush,
  children,
}: {
  title: string
  flush?: boolean
  children: React.ReactNode
}) {
  return (
    <section className="layer-card">
      <header className="layer-card-header">{title}</header>
      <div className={cx('layer-card-body', flush && 'flush')}>{children}</div>
    </section>
  )
}

function RoleBadge({ role }: { role: User['role'] }) {
  if (role === 'Owner' || role === 'Admin') {
    return (
      <span className="badge" data-tone="accent">
        {role}
      </span>
    )
  }
  return <span className={cx('badge', role === 'Visitor' && 'badge-outline')}>{role}</span>
}

export function SettingsPage() {
  const state = useAppState()
  const { theme, toggleTheme } = useTheme()
  const me = state.users.find((u) => u.id === state.currentUserId)

  const selectTheme = (target: 'dark' | 'light') => {
    if (theme !== target) toggleTheme()
  }

  return (
    <div className="page">
      <div className="pane" style={{ flex: 1 }}>
        <div className="pane-header">
          <span className="pane-title">Settings</span>
        </div>
        <div className="settings-scroll">
          <div className="settings-body">
            <SettingsCard title="Profile">
              <div className="settings-profile">
                <Avatar user={me} size={48} />
                <div style={{ minWidth: 0 }}>
                  <div className="settings-profile-name">{me?.name}</div>
                  <div className="text-muted" style={{ fontSize: 13 }}>
                    {me?.email}
                  </div>
                </div>
              </div>
              <dl className="settings-meta-rows">
                <div className="settings-meta-row">
                  <dt>Role</dt>
                  <dd>
                    <span className="badge" data-tone="accent">
                      {me?.role}
                    </span>
                  </dd>
                </div>
                <div className="settings-meta-row">
                  <dt>Title</dt>
                  <dd>{me?.title}</dd>
                </div>
              </dl>
            </SettingsCard>

            <SettingsCard title="Appearance">
              <div className="settings-row">
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="settings-row-label">Theme</div>
                  <div className="settings-row-description">
                    Dark mode is the default for the workspace
                  </div>
                </div>
                <div className="settings-segmented">
                  <button
                    type="button"
                    className="app-tab"
                    data-active={theme === 'dark' || undefined}
                    onClick={() => selectTheme('dark')}
                  >
                    Dark
                  </button>
                  <button
                    type="button"
                    className="app-tab"
                    data-active={theme === 'light' || undefined}
                    onClick={() => selectTheme('light')}
                  >
                    Light
                  </button>
                </div>
              </div>
            </SettingsCard>

            <SettingsCard title="Members" flush>
              {state.users.map((user) => (
                <div key={user.id} className="list-row" style={{ cursor: 'default' }}>
                  <Avatar user={user} size={28} showOnline />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="truncate" style={{ fontSize: 13, fontWeight: 500 }}>
                      {user.name}
                    </div>
                    <div className="truncate text-faint" style={{ fontSize: 12 }}>
                      {user.title}
                    </div>
                  </div>
                  <RoleBadge role={user.role} />
                </div>
              ))}
            </SettingsCard>

            <SettingsCard title="Features per member">
              <div className="settings-feature-rows">
                {FEATURES.map((feature) => (
                  <div key={feature} className="settings-row">
                    <span className="settings-row-label" style={{ flex: 1 }}>
                      {feature}
                    </span>
                    <span className="settings-switch" data-on="true" aria-hidden="true">
                      <span className="settings-switch-thumb" />
                    </span>
                  </div>
                ))}
                <p className="text-faint" style={{ fontSize: 12, margin: 0 }}>
                  Feature access is enforced by the server. This is a preview.
                </p>
              </div>
            </SettingsCard>

            <SettingsCard title="About">
              <dl className="settings-meta-rows" style={{ margin: 0, padding: 0, border: 'none' }}>
                <div className="settings-meta-row">
                  <dt>Version</dt>
                  <dd>0.1.0 (mock)</dd>
                </div>
                <div className="settings-meta-row">
                  <dt>Backend</dt>
                  <dd>
                    <span className="pill-dot" data-tone="warning" />
                    Not connected — using mock data
                  </dd>
                </div>
                <div className="settings-meta-row">
                  <dt>Storage</dt>
                  <dd>Local browser session</dd>
                </div>
              </dl>
            </SettingsCard>
          </div>
        </div>
      </div>
    </div>
  )
}
