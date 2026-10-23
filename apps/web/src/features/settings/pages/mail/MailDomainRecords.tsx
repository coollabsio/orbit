import { Copy } from 'reicon-react'
import type { MailDnsRecord, MailDnsRecordKind } from '@/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { copyText } from './mailSettingsLib'

const KIND_LABELS: Record<MailDnsRecordKind, string> = {
  verify: 'Verification',
  mx: 'Mail server',
  dkim: 'DKIM',
  spf: 'SPF',
  dmarc: 'DMARC',
}

/** Mail reaches the workspace only with these; DKIM, SPF and DMARC are for sending (a later release) and reputation. */
const REQUIRED: ReadonlySet<MailDnsRecordKind> = new Set(['verify', 'mx'])

/** The badge of a record from the last check: OK, or what is missing and how much it matters. */
function recordBadge(record: MailDnsRecord, checked: boolean): { label: string; variant: 'secondary' | 'destructive' | 'outline'; hint: string } {
  const required = REQUIRED.has(record.kind)
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

function CopyButton({ text, label }: { text: string; label: string }) {
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
            <TableRow key={record.kind}>
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
