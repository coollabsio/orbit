import { MailManagerGate, useMailManager } from '../../../../components/settings/mail/DomainGate'
import { ProviderConnections } from '../../../../components/settings/mail/ProviderConnections'
import { SettingsShell, WORKSPACE_NAV } from '../../../../components/settings/Shell'
import { useMailProviders, useMailSendRoutes } from '../../../../hooks/mail-domains'

/** The web's Settings → Mail → Providers: the workspace's incoming providers and outgoing routes, and the server's ones it may use. */
export default function MailProvidersScreen() {
  const manager = useMailManager()
  const providers = useMailProviders('workspace', manager)
  const routes = useMailSendRoutes('workspace', manager)
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={manager ? () => Promise.all([providers.refetch(), routes.refetch()]) : undefined}>
      <MailManagerGate title="Providers">
        <ProviderConnections scope="workspace" />
      </MailManagerGate>
    </SettingsShell>
  )
}
