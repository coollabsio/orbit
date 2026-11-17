import { useState } from 'react'
import type { MailProviderSetup, MailProviderView, MailSendRouteView } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Danger } from 'reicon-react'
import { useSendRoutes, type ProviderScope } from '@/features/mail/api/queries'
import { CopyButton } from './MailDomainRecords'
import { RouteCreatedModal } from './MailSendingPage'
import { ProviderSendRouteForm } from './SendRouteForm'

const SECRET_LABELS: Record<string, string> = {
  signing_secret: 'Signing secret',
  basic_user: 'User name',
  basic_password: 'Password',
}

/** One value to paste at the provider, with a copy button. */
function CopyRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <div className="flex items-start gap-1 rounded-md border bg-muted/40 px-2 py-1">
        <code className="min-w-0 flex-1 py-0.5 font-mono text-xs break-all">{value}</code>
        <CopyButton text={value} label={label} />
      </div>
    </div>
  )
}

/**
 * What to set up at the provider: the webhook URL, the steps and (Cloudflare) the Email Worker. `generated` holds the
 * secrets Orbit made, which only the create response has. With `provider`: **Also send through this provider**.
 */
export function ProviderSetup({
  setup,
  generated,
  provider,
  scope,
}: {
  setup: MailProviderSetup
  generated?: Record<string, string> | null
  provider?: MailProviderView
  scope?: ProviderScope
}) {
  const secrets = Object.entries(generated ?? {})
  return (
    <div className="flex flex-col gap-4">
      {secrets.length > 0 ? (
        <Alert>
          <Danger />
          <AlertTitle>Copy these now. Orbit does not show them again.</AlertTitle>
          <AlertDescription>If you lose them, remove the provider and add it again.</AlertDescription>
        </Alert>
      ) : null}
      <CopyRow label="Webhook URL" value={setup.webhook_url} />
      {secrets.map(([name, value]) => <CopyRow key={name} label={SECRET_LABELS[name] ?? name} value={value} />)}
      <ol className="m-0 flex list-decimal flex-col gap-1.5 pl-5 text-[0.8125rem]">
        {setup.steps.map((step) => <li key={step}>{step}</li>)}
      </ol>
      {setup.worker_script ? (
        <div className="flex flex-col gap-1">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-medium text-muted-foreground">Email Worker script</span>
            <CopyButton text={setup.worker_script} label="Script" />
          </div>
          <pre className="m-0 max-h-72 overflow-auto rounded-md border bg-muted/40 p-2 font-mono text-xs">{setup.worker_script}</pre>
        </div>
      ) : null}
      {provider && scope ? <ProviderSendRoute provider={provider} scope={scope} /> : null}
    </div>
  )
}

/** **Also send through this provider**, or the route that already does. */
function ProviderSendRoute({ provider, scope }: { provider: MailProviderView; scope: ProviderScope }) {
  const routes = useSendRoutes(scope)
  const [adding, setAdding] = useState(false)
  const [created, setCreated] = useState<MailSendRouteView | null>(null)
  if (!routes.data) return null
  const route = routes.data.routes.find((entry) => entry.provider_id === provider.id)
  const allowed = scope === 'instance' || routes.data.workspace_providers_enabled
  if (!route && !allowed) return null
  return (
    <div className="flex flex-wrap items-center gap-2 border-t pt-3">
      <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
        {route ? `Also sends mail through the route ${route.name}` : 'Send mail too'}
        <InfoTip text="A send route with the same account, so your domains can send through this provider. Orbit asks only for what the provider does not have yet." />
      </span>
      {route ? null : <Button type="button" variant="outline" size="sm" onClick={() => setAdding(true)}>Also send through this provider</Button>}
      {adding ? <ProviderSendRouteForm scope={scope} provider={provider} onClose={() => setAdding(false)} onCreated={setCreated} /> : null}
      {created ? <RouteCreatedModal created={created} onClose={() => setCreated(null)} /> : null}
    </div>
  )
}
