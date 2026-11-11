import { useState } from 'react'
import { toast } from 'sonner'
import { Add, InfoCircle, Routing } from 'reicon-react'
import type { MailProviderView } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useDeleteProvider, useUpdateProvider } from '@/features/mail/api/mutations'
import { useMailProviders, type ProviderScope } from '@/features/mail/api/queries'
import { useCan } from '@/features/workspaces/permissions'
import { MailManagersOnly } from './MailDomainsPage'
import { count, mailProblem, PROVIDER_LABELS, since } from './mailSettingsLib'
import { ProviderForm } from './ProviderForm'
import { ProviderSetup } from './ProviderSetup'

/** Settings → Mail → Providers: the workspace's own providers, and the instance's ones it may use. */
export function MailProvidersPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Mail providers" />
  return <MailProviders scope="workspace" />
}

/**
 * The mail providers of `scope` (Admin → Mail for `instance`): a provider receives a domain's mail and sends it to
 * Orbit over HTTPS. Each own provider shows what to set up at the provider; secrets Orbit makes show once.
 */
export function MailProviders({ scope }: { scope: ProviderScope }) {
  const providers = useMailProviders(scope)
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<MailProviderView | null>(null)
  // a provider with secrets that Orbit just made: shown once
  const [shown, setShown] = useState<{ provider: MailProviderView; title: string } | null>(null)
  const canAdd = scope === 'instance' || providers.data?.workspace_providers_enabled === true

  return (
    <>
      {scope === 'workspace' && providers.data && !providers.data.workspace_providers_enabled ? (
        <Alert>
          <InfoCircle />
          <AlertTitle>Your server admin has turned off workspace providers.</AlertTitle>
          <AlertDescription>Your domains can still use the providers your server admin set up, listed below.</AlertDescription>
        </Alert>
      ) : null}
      <SettingsCard
        title="Mail providers"
        description={
          scope === 'instance'
            ? 'Providers that every workspace can route its domains to. Use one when this server cannot receive on port 25.'
            : 'Receive a domain through your own account at PostShiba, Cloudflare, Postmark, Mailgun, Resend or Amazon SES, when this server cannot receive on port 25.'
        }
        actions={canAdd ? <Button type="button" onClick={() => setAdding(true)}><Add />Add provider</Button> : undefined}
        flush
      >
        {providers.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading providers…</p> : null}
        {providers.isError ? (
          <p role="alert" className="m-0 p-4 text-sm text-destructive">
            {mailProblem(providers.error, 'The providers could not be loaded.')}{' '}
            <Button variant="ghost" onClick={() => void providers.refetch()}>Retry</Button>
          </p>
        ) : null}
        {providers.data?.providers.length === 0 ? (
          <EmptyState icon={Routing} title="No providers yet" description="Without a provider, mail for your domains arrives through this server's SMTP listener." />
        ) : null}
        <div className="flex flex-col divide-y">
          {providers.data?.providers.map((provider) => (
            <ProviderRow
              key={provider.id}
              scope={scope}
              provider={provider}
              onEdit={() => setEditing(provider)}
              onNewSecret={(changed) => setShown({ provider: changed, title: `New secret for ${changed.name}` })}
            />
          ))}
        </div>
      </SettingsCard>
      {adding ? (
        <ProviderForm scope={scope} onClose={() => setAdding(false)} onCreated={(created) => setShown({ provider: created, title: `Set up ${created.name}` })} />
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
      {open && provider.setup ? <ProviderSetup setup={provider.setup} /> : null}
    </div>
  )
}
