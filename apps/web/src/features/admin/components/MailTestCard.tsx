import { useState } from 'react'
import { Copy } from 'reicon-react'
import type { MailAdmin, MailAuthResult, MailTestAddress, MailTestResult } from '@/api/generated/types.gen'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Tip } from '@/components/common/Tip'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { useCreateTestAddress } from '@/features/mail/api/mutations'
import { useTestAddress } from '@/features/mail/api/queries'
import { copyText, dateTime, mailProblem } from '@/features/settings/pages/mail/mailSettingsLib'
import { timeOfDay } from '@/lib/format'

const RESULT_VARIANTS: Partial<Record<MailAuthResult, 'secondary' | 'destructive'>> = { pass: 'secondary', fail: 'destructive', permerror: 'destructive' }

function AuthBadge({ check, result }: { check: string; result: MailAuthResult }) {
  return <Badge variant={RESULT_VARIANTS[result] ?? 'outline'}>{check}: {result}</Badge>
}

/**
 * **Test message**: a one-time address `check-…@<verified domain>`, valid for an hour. Mail sent to it from outside
 * proves MX, port 25, TLS and the checks; the card asks for the result every 3 s until it arrives or expires.
 */
export function MailTestCard({ admin }: { admin: MailAdmin }) {
  const create = useCreateTestAddress()
  const [domainId, setDomainId] = useState(admin.test_domains[0]?.id ?? '')
  const [test, setTest] = useState<MailTestAddress | null>(null)
  const result = useTestAddress(test?.id)
  const options = admin.test_domains.map((domain) => ({ value: domain.id, label: domain.name }))
  const chosen = options.some((option) => option.value === domainId) ? domainId : (options[0]?.value ?? '')
  const unavailable = !admin.enabled
    ? 'Turn mail on for this server first (ORBIT_MAIL_ENABLED).'
    : options.length === 0
      ? 'No workspace has a verified domain yet. Add one in Settings → Mail → Domains of a workspace.'
      : null

  return (
    <SettingsCard
      title="Test message"
      description="Send a mail from an outside account (for example Gmail) to a one-time address. It does not go to a mailbox; the card shows what Orbit saw."
      flush
    >
      <div className="flex flex-col divide-y">
        <div className="flex flex-wrap items-center gap-2 px-4 py-3">
          {unavailable ? <p className="m-0 flex-1 text-[13px] text-muted-foreground">{unavailable}</p> : (
            <>
              <Select items={options} value={chosen} onValueChange={(value) => setDomainId(value as string)}>
                <SelectTrigger aria-label="Domain of the test address" className="w-56"><SelectValue /></SelectTrigger>
                <SelectContent>
                  {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                </SelectContent>
              </Select>
              <Button type="button" disabled={!chosen || create.isPending} onClick={() => create.mutate(chosen, { onSuccess: setTest })}>
                {create.isPending ? 'Making an address…' : test ? 'New test address' : 'Make a test address'}
              </Button>
              {create.isError ? <p role="alert" className="m-0 text-xs text-destructive">{mailProblem(create.error, 'The test address could not be made.')}</p> : null}
            </>
          )}
        </div>
        {test ? <TestResult test={test} result={result.data} failed={result.isError} /> : null}
      </div>
    </SettingsCard>
  )
}

function TestResult({ test, result, failed }: { test: MailTestAddress; result: MailTestResult | undefined; failed: boolean }) {
  const status = result?.status ?? 'waiting'
  return (
    <div className="flex flex-col gap-3 px-4 py-3 text-[13px]" data-slot="test-result" data-status={status}>
      <div className="flex flex-wrap items-center gap-2">
        <code className="font-mono">{test.address}</code>
        <Tip label="Copy address">
          <Button type="button" variant="ghost" size="icon-xs" aria-label="Copy address" onClick={() => copyText(test.address, 'Address')}><Copy /></Button>
        </Tip>
        <span className="text-xs text-muted-foreground">Valid until {timeOfDay(test.expires_at)}</span>
      </div>
      {status === 'waiting' ? (
        <p role="status" className="m-0 flex items-center gap-2 text-muted-foreground">
          {failed ? null : <Spinner role="presentation" aria-hidden aria-label={undefined} className="size-3.5" />}
          {failed ? 'The result could not be read. Orbit tries again.' : 'Waiting for the message…'}
        </p>
      ) : null}
      {status === 'expired' ? <p role="status" className="m-0 text-muted-foreground">The address expired before a message arrived. Make a new one.</p> : null}
      {status === 'received' && result ? (
        <div role="status" className="flex flex-col gap-2">
          <p className="m-0 font-medium text-emerald-700 dark:text-emerald-400">The message arrived{result.received_at ? ` at ${dateTime(result.received_at)}` : ''}.</p>
          <dl className="m-0 grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-xs">
            <dt className="text-muted-foreground">Client address</dt>
            <dd className="m-0 font-mono">{result.peer_ip ?? 'unknown'}</dd>
            <dt className="text-muted-foreground">HELO</dt>
            <dd className="m-0 font-mono break-all">{result.helo ?? '—'}</dd>
            <dt className="text-muted-foreground">Sender</dt>
            <dd className="m-0 font-mono break-all">{result.mail_from ?? '<>'}</dd>
            <dt className="text-muted-foreground">TLS</dt>
            <dd className="m-0">{result.tls ? `${result.tls.version} (${result.tls.cipher})` : 'None: the message came without TLS'}</dd>
          </dl>
          {result.auth ? (
            <div className="flex flex-wrap items-center gap-1.5">
              <AuthBadge check="SPF" result={result.auth.spf} />
              <AuthBadge check="DKIM" result={result.auth.dkim} />
              <AuthBadge check="DMARC" result={result.auth.dmarc} />
              {result.auth.aligned_domain ? <span className="text-xs text-muted-foreground">Aligned with {result.auth.aligned_domain}</span> : null}
            </div>
          ) : <p className="m-0 text-xs text-muted-foreground">No authentication results were recorded.</p>}
        </div>
      ) : null}
    </div>
  )
}
