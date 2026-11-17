import { toast } from 'sonner'
import { Copy, Refresh2 } from 'reicon-react'
import type { MailDnsRecord, MailDomain } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { Tip } from '@/components/common/Tip'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useRotateDkim } from '@/features/mail/api/mutations'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { copyText, isSendRouteKind, KIND_LABELS, mailProblem, REQUIRED, routeLabel, routeRecordsNote, SEND_REQUIRED, spfHint } from './mailSettingsLib'

/** The badge of a record from the last check: OK, or what is missing and how much it matters. */
function recordBadge(record: MailDnsRecord, checked: boolean): { label: string; variant: 'secondary' | 'destructive' | 'outline'; hint: string } {
  const required = REQUIRED.has(record.kind)
  const sending = SEND_REQUIRED.has(record.kind)
  if (sending && (record.status.status === 'missing' || record.status.status === 'wrong')) {
    return {
      label: 'Required for sending',
      variant: 'destructive',
      hint: record.status.status === 'missing'
        ? 'The record was not found. Orbit signs sent mail with this key and sends nothing without it.'
        : 'The record has another value. Orbit signs sent mail with this key and sends nothing until it matches.',
    }
  }
  switch (record.status.status) {
    case 'ok':
      return { label: 'OK', variant: 'secondary', hint: 'The record was found with this value.' }
    case 'unknown':
      return checked
        ? { label: 'Unknown', variant: 'outline', hint: 'The DNS lookup did not answer. Orbit tries again on the next check.' }
        : { label: 'Not checked', variant: 'outline', hint: 'Orbit has not checked the DNS yet.' }
    case 'missing':
      return required
        ? { label: 'Required', variant: 'destructive', hint: 'The record was not found. Mail needs it.' }
        : { label: 'Recommended', variant: 'outline', hint: 'The record was not found. Add it so other mail servers trust the domain.' }
    case 'wrong':
      return required
        ? { label: 'Required', variant: 'destructive', hint: 'The record has another value. Mail needs this one.' }
        : { label: 'Warning', variant: 'outline', hint: 'The record has another value. Replace it with this one.' }
  }
}

export function CopyButton({ text, label }: { text: string; label: string }) {
  return (
    <Tip label={`Copy ${label.toLowerCase()}`}>
      <Button type="button" variant="ghost" size="icon-xs" aria-label={`Copy ${label.toLowerCase()}`} onClick={() => copyText(text, label)}>
        <Copy />
      </Button>
    </Tip>
  )
}

/** The records to publish for a domain, each with copy buttons and its status at the last check (`checked`). */
export function MailDomainRecords({ records, checked }: { records: MailDnsRecord[]; checked: boolean }) {
  return (
    <Table aria-label="DNS records">
      <TableHeader>
        <TableRow>
          <TableHead>Type</TableHead>
          <TableHead>Name</TableHead>
          <TableHead>Value</TableHead>
          <TableHead>Status</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {records.map((record) => {
          const badge = recordBadge(record, checked)
          const found = record.status.status === 'wrong' ? record.status.found : null
          return (
            <TableRow key={`${record.kind}:${record.value}`}>
              <TableCell className="align-top">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">{record.type}</span>
                  <span className="text-xs text-muted-foreground">{KIND_LABELS[record.kind]}</span>
                </div>
              </TableCell>
              <TableCell className="align-top">
                <div className="flex items-start gap-1">
                  <code className="min-w-0 py-0.5 font-mono text-xs break-all whitespace-normal">{record.name}</code>
                  <CopyButton text={record.name} label="Name" />
                </div>
              </TableCell>
              <TableCell className="max-w-md align-top">
                <div className="flex items-start gap-1">
                  <div className="flex min-w-0 flex-1 flex-col gap-1">
                    <code className="py-0.5 font-mono text-xs break-all whitespace-normal">{record.value}</code>
                    {record.priority != null ? <span className="text-xs text-muted-foreground">Priority {record.priority}</span> : null}
                    {record.txt_strings.length > 1 ? (
                      <div className="flex flex-col gap-0.5 text-xs whitespace-normal text-muted-foreground">
                        <span>Longer than one TXT string. Most DNS panels take the value as it is; zone files need {record.txt_strings.length} strings:</span>
                        <code className="font-mono break-all">{record.txt_strings.map((part) => `"${part}"`).join(' ')}</code>
                      </div>
                    ) : null}
                  </div>
                  <CopyButton text={record.value} label="Value" />
                </div>
              </TableCell>
              <TableCell className="align-top">
                <div className="flex max-w-56 flex-col items-start gap-1 whitespace-normal">
                  <Badge variant={badge.variant} title={badge.hint}>{badge.label}</Badge>
                  {found !== null ? <span className="text-xs break-all text-muted-foreground">Found: <code className="font-mono">{found || '(empty)'}</code></span> : null}
                </div>
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}

/** The DKIM selector of the domain's record (`o2026a` of `o2026a._domainkey.example.com`). */
const selectorOf = (record: MailDnsRecord) => record.name.split('._domainkey.')[0] ?? record.name

/**
 * A domain's sending: the route in use and the records its provider needs besides Orbit's, and **Rotate key** for
 * Orbit's DKIM key.
 */
export function DomainSendRecords({ domain }: { domain: MailDomain }) {
  const rotate = useRotateDkim()
  const dkim = domain.records.find((record) => record.kind === 'dkim')
  const kind = domain.send_route && isSendRouteKind(domain.send_route.kind) ? domain.send_route.kind : null
  const note = kind ? routeRecordsNote(kind, domain.name) : null
  const spf = kind && kind !== 'direct' ? spfHint(kind, null) : null

  async function rotateKey() {
    const confirmed = await confirmAction({
      title: `Make a new DKIM key for ${domain.name}?`,
      description: 'Orbit keeps signing with the current key until the new record is published and found by the DNS check. Then it switches, and you can delete the old record a few days later.',
      confirmLabel: 'Make new key',
    })
    if (!confirmed) return
    rotate.mutate(domain.id, {
      onSuccess: () => toast('New DKIM key made. Publish its record below, then click Check now.'),
      onError: (error) => toast.error(mailProblem(error, 'The new key could not be made.')),
    })
  }

  return (
    <SettingsRow className="flex-nowrap">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
        <span className="flex items-center gap-1.5 font-medium text-foreground">
          Sending
          <InfoTip text="Orbit signs every message with the domain's DKIM key: the DKIM record below is required for sending. SPF and DMARC help your mail reach the inbox." />
        </span>
        <span className="text-xs text-muted-foreground">
          {domain.send_route ? `Sends through ${routeLabel(domain.send_route.kind)}.` : 'No send route: this domain cannot send. Pick one above, or ask your server admin for a default route.'}
          {spf ? <> SPF includes <code className="font-mono">{spf}</code>.</> : null}
          {note ? ` ${note}` : null}
        </span>
      </div>
      {dkim ? (
        <Tip label={`Make a new key for selector ${selectorOf(dkim)}`}>
          <Button type="button" variant="outline" disabled={rotate.isPending} onClick={() => void rotateKey()}>
            <Refresh2 />
            {rotate.isPending ? 'Making key…' : 'Rotate key'}
          </Button>
        </Tip>
      ) : null}
    </SettingsRow>
  )
}
