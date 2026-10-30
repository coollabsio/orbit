import type { MailProviderSetup } from '@/api/generated/types.gen'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Danger } from 'reicon-react'
import { CopyButton } from './MailDomainRecords'

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
 * secrets Orbit made, which only the create response has.
 */
export function ProviderSetup({ setup, generated }: { setup: MailProviderSetup; generated?: Record<string, string> | null }) {
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
      <ol className="m-0 flex list-decimal flex-col gap-1.5 pl-5 text-[13px]">
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
    </div>
  )
}
