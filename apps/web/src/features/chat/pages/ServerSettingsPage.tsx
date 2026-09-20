// Chat settings: a settings sidebar replaces the channel sidebar (same width), with
// Roles / Webhooks / Emoji tabs; the content column fills the rest.
import { useState } from 'react'
import { useSearchParams } from 'react-router'
import { Smile, Users } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { WebhookIcon } from '@/components/common/icons/WebhookIcon'
import { EmojiTab } from '@/features/chat/components/settings/EmojiTab'
import { RolesTab } from '@/features/chat/components/settings/RolesTab'
import { WebhooksTab } from '@/features/chat/components/settings/WebhooksTab'

type Tab = 'roles' | 'webhooks' | 'emoji'

const navItems: { key: Tab; label: string; icon: React.ComponentType<{ className?: string }>; danger?: boolean }[] = [
  { key: 'roles', label: 'Roles', icon: Users },
  { key: 'webhooks', label: 'Webhooks', icon: WebhookIcon },
  { key: 'emoji', label: 'Emoji', icon: Smile },
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
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background max-[899px]:flex-col">
      {/* settings sidebar — replaces the channel sidebar, same width */}
      <div className="flex w-60 shrink-0 flex-col border-r border-border bg-background max-[899px]:w-full max-[899px]:border-r-0 max-[899px]:border-b">
        <div className="flex min-h-0 flex-1 flex-col overflow-auto">
          <div className="flex min-w-0 flex-col gap-0.5 px-4 pt-7 max-[899px]:p-3">
            <div className="flex items-center gap-2 pl-0.5 text-xs leading-5 font-semibold text-muted-foreground max-[899px]:hidden">CHAT SETTINGS</div>
            <div className="grid gap-0.5 max-[899px]:flex max-[899px]:gap-1 max-[899px]:overflow-x-auto">
              {navItems.map(({ key, label, icon: Icon, danger }) => (
                <Button
                  key={key}
                  type="button"
                  variant="ghost"
                  className={cn(
                    'flex h-auto min-w-0 items-center justify-start gap-2 rounded-lg border-0 p-2 text-left text-sm leading-5 font-medium text-foreground transition-colors',
                    '[&_svg]:shrink-0 [&_svg]:text-muted-foreground',
                    'hover:bg-sidebar-accent/50 hover:text-foreground dark:hover:bg-sidebar-accent/50 data-[active=true]:bg-sidebar-accent data-[active=true]:[&_svg]:text-primary',
                    'data-[danger=true]:text-destructive data-[danger=true]:[&_svg]:text-destructive data-[danger=true]:hover:bg-destructive/10 data-[danger=true]:hover:text-destructive data-[danger=true]:data-[active=true]:bg-destructive/10',
                    'max-[899px]:min-w-max max-[899px]:px-3 max-[899px]:py-2',
                  )}
                  data-active={activeTab === key ? 'true' : undefined}
                  data-danger={danger ? 'true' : undefined}
                  onClick={() => selectTab(key)}
                >
                  <Icon className="size-4" />
                  <span className="truncate">{label}</span>
                </Button>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* content area — fills the rest */}
      <div className="min-w-0 flex-1 overflow-auto bg-background">
        <div className="max-w-2xl p-10 data-[wide=true]:max-w-5xl max-[899px]:p-5" data-wide={activeTab === 'roles' ? 'true' : undefined}>
          {activeTab === 'roles' ? <RolesTab /> : null}
          {activeTab === 'webhooks' ? <WebhooksTab /> : null}
          {activeTab === 'emoji' ? <EmojiTab /> : null}
        </div>
      </div>
    </div>
  )
}
