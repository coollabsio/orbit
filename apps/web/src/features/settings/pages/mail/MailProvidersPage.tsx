import { useState } from 'react'
import { toast } from 'sonner'
import { Add, DirectInbox, InfoCircle, Send2 } from 'reicon-react'
import type { MailProviderView } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useDeleteProvider, useUpdateProvider } from '@/features/mail/api/mutations'
import { useMailProviders, type ProviderScope } from '@/features/mail/api/queries'
import { useCan } from '@/features/workspaces/permissions'
import { MailManagersOnly } from './MailDomainsPage'
import { count, mailProblem, PROVIDER_LABELS, since } from './mailSettingsLib'
import { ProviderForm } from './ProviderForm'
import { ProviderSetup } from './ProviderSetup'
import { OutgoingRoutes } from './MailSendingPage'

/** Settings → Mail → Providers: the workspace's incoming providers and outgoing routes, and the server's ones it may use. */
export function MailProvidersPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Providers" />
  return <MailConnections scope="workspace" />
}

type Direction = 'incoming' | 'outgoing'

/**
 * One card for the providers of `scope` (Admin → Mail for `instance`): incoming providers receive a domain's mail and
 * pass it to Orbit, outgoing routes send it. **Add provider** asks which one. `hostname` fills the SPF of a direct route.
 */
export function MailConnections({ scope, hostname }: { scope: ProviderScope; hostname?: string | null }) {
  const providers = useMailProviders(scope)
  const [adding, setAdding] = useState<Direction | null>(null)
  const turnedOff = scope === 'workspace' && providers.data !== undefined && !providers.data.workspace_providers_enabled
  const canAdd = scope === 'instance' || providers.data?.workspace_providers_enabled === true

  return (
    <>
      {turnedOff ? (
        <Alert>
          <InfoCircle />
          <AlertTitle>Your server admin has turned off workspace providers.</AlertTitle>
          <AlertDescription>Your domains can still use the providers your server admin set up, listed below.</AlertDescription>
        </Alert>
      ) : null}
      <SettingsCard
        title="Providers"
        description={
          scope === 'instance'
            ? 'Incoming providers receive mail for the domains and pass it to Orbit: use one when this server cannot receive on port 25. Outgoing providers send mail: directly from this server, through an SMTP relay or through a provider. Every workspace can pick these for its domains. Domains without an outgoing provider send through the default one; without a default, they cannot send.'
            : 'Incoming providers receive mail for your domains through your own account, when this server cannot receive on port 25. Outgoing providers send it. Pick them for each domain on its card in Domains; without an outgoing one, a domain uses the server’s default.'
        }
        actions={
          canAdd ? (
            <DropdownMenu modal={false}>
              <DropdownMenuTrigger render={<Button type="button"><Add />Add provider</Button>} />
              <DropdownMenuContent align="end" className="w-auto min-w-56">
                <DropdownMenuItem onClick={() => setAdding('incoming')}><DirectInbox />Incoming: receive mail</DropdownMenuItem>
                <DropdownMenuItem onClick={() => setAdding('outgoing')}><Send2 />Outgoing: send mail</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : undefined
        }
        flush
      >
        <div className="flex flex-col divide-y">
          <SectionHeading>Incoming</SectionHeading>
          <IncomingProviders scope={scope} adding={adding === 'incoming'} onAddClose={() => setAdding(null)} />
          <SectionHeading>Outgoing</SectionHeading>
          <OutgoingRoutes scope={scope} hostname={hostname} adding={adding === 'outgoing'} onAddClose={() => setAdding(null)} />
        </div>
      </SettingsCard>
    </>
  )
}

function SectionHeading({ children }: { children: string }) {
  return <h3 className="m-0 bg-muted/40 px-4 py-2 text-xs font-medium text-muted-foreground">{children}</h3>
}

/** The incoming providers of `scope`. Each own provider shows what to set up at the provider; secrets Orbit makes show once. */
function IncomingProviders({ scope, adding, onAddClose }: { scope: ProviderScope; adding: boolean; onAddClose: () => void }) {
  const providers = useMailProviders(scope)
  const [editing, setEditing] = useState<MailProviderView | null>(null)
  // a provider with secrets that Orbit just made: shown once
  const [shown, setShown] = useState<{ provider: MailProviderView; title: string } | null>(null)

  return (
    <>
      {providers.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading providers…</p> : null}
      {providers.isError ? (
        <p role="alert" className="m-0 p-4 text-sm text-destructive">
          {mailProblem(providers.error, 'The providers could not be loaded.')}{' '}
          <Button variant="ghost" onClick={() => void providers.refetch()}>Retry</Button>
        </p>
      ) : null}
      {providers.data ? <ListenerRow running={providers.data.listener.running} hostname={providers.data.listener.hostname} /> : null}
      {providers.data?.providers.map((provider) => (
        <ProviderRow
          key={provider.id}
          scope={scope}
          provider={provider}
          onEdit={() => setEditing(provider)}
          onNewSecret={(changed) => setShown({ provider: changed, title: `New secret for ${changed.name}` })}
        />
      ))}
      {adding ? (
        <ProviderForm scope={scope} onClose={onAddClose} onCreated={(created) => setShown({ provider: created, title: `Set up ${created.name}` })} />
      ) : null}
      {editing ? <ProviderForm scope={scope} provider={editing} onClose={() => setEditing(null)} /> : null}
      {shown?.provider.setup ? (
        <Modal title={shown.title} description={`Do these steps at ${PROVIDER_LABELS[shown.provider.kind]}.`} onClose={() => setShown(null)}>
          <ProviderSetup setup={shown.provider.setup} generated={shown.provider.generated_secrets} />
        </Modal>
      ) : null}
    </>
  )
}

/** This server's SMTP listener: domains without an incoming provider receive through it. */
function ListenerRow({ running, hostname }: { running: boolean; hostname?: string | null }) {
  return (
    <div className="flex flex-col gap-0.5 px-4 py-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[0.8125rem] font-medium text-foreground">SMTP listener</span>
        <Badge variant="secondary">Server</Badge>
        <Badge variant={running ? 'outline' : 'destructive'}>{running ? 'Running' : 'Off'}</Badge>
      </div>
      <span className="text-xs text-muted-foreground">
        {running
          ? `Receives mail for domains without an incoming provider${hostname ? ` (MX: ${hostname})` : ''}.`
          : 'This server does not receive mail itself. Domains need an incoming provider.'}
      </span>
    </div>
  )
}

function ProviderRow({
  scope,
  provider,
  onEdit,
  onNewSecret,
}: {
  scope: ProviderScope
  provider: MailProviderView
  onEdit: () => void
  onNewSecret: (changed: MailProviderView) => void
}) {
  const remove = useDeleteProvider(scope)
  const update = useUpdateProvider(scope)
  // Orbit makes these secrets, so Orbit can replace them
  const madeByOrbit = provider.kind === 'cloudflare' || provider.kind === 'postmark'
  const [open, setOpen] = useState(false)
  const own = provider.setup !== null && provider.setup !== undefined

  async function removeProvider() {
    const confirmed = await confirmAction({
      title: `Remove ${provider.name}?`,
      description: 'Orbit stops accepting mail from it. Remove it at the provider too.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(provider.id, {
      onSuccess: () => toast(`${provider.name} removed`),
      onError: (error) => toast.error(mailProblem(error, 'The provider could not be removed.')),
    })
  }

  async function newSecret() {
    const confirmed = await confirmAction({
      title: `Make a new secret for ${provider.name}?`,
      description:
        provider.kind === 'cloudflare'
          ? 'The old signing secret stops working now. Mail is refused until you set the new one on the Worker.'
          : 'The old credentials stop working now. Mail is refused until you set the new webhook URL in Postmark.',
      confirmLabel: 'Make new secret',
      danger: true,
    })
    if (!confirmed) return
    update.mutate(
      { providerId: provider.id, body: { regenerate_secrets: true } },
      {
        onSuccess: onNewSecret,
        onError: (error) => toast.error(mailProblem(error, 'The new secret could not be made.')),
      },
    )
  }

  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.8125rem] font-medium text-foreground">{provider.name}</span>
            <Badge variant="outline">{PROVIDER_LABELS[provider.kind]}</Badge>
            {scope === 'workspace' && provider.scope === 'instance' ? <Badge variant="secondary">Server</Badge> : null}
          </div>
          <span className="text-xs text-muted-foreground">
            {count(provider.domain_count, 'domain', 'domains')}
            {provider.last_received_at ? ` · Last message ${since(provider.last_received_at)}` : ' · No message yet'}
          </span>
          {provider.last_error ? <span role="status" className="text-xs text-destructive">Last error: {provider.last_error}</span> : null}
        </div>
        {own ? (
          <div className="flex items-center gap-1">
            <Button type="button" variant="outline" size="sm" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? 'Hide setup' : 'Setup'}</Button>
            <Button type="button" variant="ghost" size="sm" onClick={onEdit}>Change</Button>
            {madeByOrbit ? (
              <Button type="button" variant="ghost" size="sm" disabled={update.isPending} onClick={() => void newSecret()}>New secret</Button>
            ) : null}
            <Button type="button" variant="ghost" size="sm" className="text-destructive" disabled={remove.isPending} onClick={() => void removeProvider()}>Remove</Button>
          </div>
        ) : null}
      </div>
      {open && provider.setup ? <ProviderSetup setup={provider.setup} provider={provider} scope={scope} /> : null}
    </div>
  )
}
