// Chat settings: a settings sidebar replaces the channel sidebar (same width), with
// Roles / Webhooks / Emoji tabs; the content column fills the rest.
import { useState } from 'react'
import { useSearchParams } from 'react-router'
import { EmojiHappy, People } from 'reicon-react'
import { WebhookIcon } from '../../components/ui/WebhookIcon'
import { EmojiTab } from './settings/EmojiTab'
import { RolesTab } from './settings/RolesTab'
import { WebhooksTab } from './settings/WebhooksTab'
import './chat.css'
import './settings/server.css'

type Tab = 'roles' | 'webhooks' | 'emoji'

const navItems: { key: Tab; label: string; icon: React.ComponentType<{ size?: number }>; danger?: boolean }[] = [
  { key: 'roles', label: 'Roles', icon: People },
  { key: 'webhooks', label: 'Webhooks', icon: WebhookIcon },
  { key: 'emoji', label: 'Emoji', icon: EmojiHappy },
]

function isTab(value: string | null): value is Tab {
  return value === 'roles' || value === 'webhooks' || value === 'emoji'
}

export function ServerSettingsPage() {
  const [searchParams, setSearchParams] = useSearchParams()
  const [activeTab, setActiveTab] = useState<Tab>(() => (isTab(searchParams.get('tab')) ? (searchParams.get('tab') as Tab) : 'roles'))

  function selectTab(tab: Tab) {
    setActiveTab(tab)
    const next = new URLSearchParams(searchParams)
    if (tab === 'roles') next.delete('tab')
    else next.set('tab', tab)
    setSearchParams(next, { replace: true })
  }

  return (
    <div className="page chat-page ss-page">
      {/* settings sidebar — replaces the channel sidebar, same width */}
      <div className="ss-sidebar">
        <div className="ss-nav-scroll">
          <div className="ss-nav">
            <div className="ss-nav-label">CHAT SETTINGS</div>
            <div className="ss-nav-items">
              {navItems.map(({ key, label, icon: Icon, danger }) => (
                <button
                  key={key}
                  type="button"
                  className="ss-nav-item"
                  data-active={activeTab === key ? 'true' : undefined}
                  data-danger={danger ? 'true' : undefined}
                  onClick={() => selectTab(key)}
                >
                  <Icon size={16} />
                  <span className="truncate">{label}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* content area — fills the rest */}
      <div className="ss-content">
        <div className="ss-inner" data-wide={activeTab === 'roles' ? 'true' : undefined}>
          {activeTab === 'roles' ? <RolesTab /> : null}
          {activeTab === 'webhooks' ? <WebhooksTab /> : null}
          {activeTab === 'emoji' ? <EmojiTab /> : null}
        </div>
      </div>
    </div>
  )
}
