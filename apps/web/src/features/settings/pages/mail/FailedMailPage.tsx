import { toast } from 'sonner'
import { SmsTracking } from 'reicon-react'
import type { MailFailedReceipt, MailRejectedDelivery, MailRejectReason } from '@/api/generated/types.gen'
import { EmptyState } from '@/components/common/EmptyState'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useRetryReceipt } from '@/features/mail/api/mutations'
import { useFailedReceipts } from '@/features/mail/api/queries'
import { useCan } from '@/features/workspaces/permissions'
import { formatSize } from '@/lib/attachmentLib'
import { MailManagersOnly } from './MailDomainsPage'
import { count, dateTime, mailProblem } from './mailSettingsLib'

/**
 * Messages Orbit accepted but could not file into their mailboxes (`failed`), or that have waited for over an hour
 * without a job (`stuck`). The raw message is kept; **Retry** files it again.
 */
export function FailedMailPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Failed messages" />
  return <FailedMail />
}

function FailedMail() {
  const receipts = useFailedReceipts()
  if (receipts.isPending) return <SettingsCard title="Failed messages"><p role="status">Loading messages…</p></SettingsCard>
  if (receipts.isError) {
    return (
      <SettingsCard title="Failed messages">
        <p role="alert">{mailProblem(receipts.error, 'Failed messages could not be loaded.')} <Button variant="ghost" onClick={() => void receipts.refetch()}>Retry</Button></p>
      </SettingsCard>
    )
  }
  const { count: total, receipts: rows, rejected } = receipts.data
  if (total === 0 && rejected.length === 0) {
    return <EmptyState icon={SmsTracking} title="No failed messages" description="Every message Orbit accepted reached its mailboxes." />
  }
  const shown = rows.length < total ? ` The newest ${rows.length} are shown.` : ''
  return (
    <>
      {total > 0 ? <FailedReceipts total={total} rows={rows} shown={shown} /> : null}
      {rejected.length > 0 ? <NotDelivered rejected={rejected} /> : null}
    </>
  )
}

const REASON_TEXTS: Record<MailRejectReason, string> = {
  unknown_address: 'No mailbox has this address',
  wrong_route: 'This domain does not receive through this provider',
  domain_unavailable: 'The domain is suspended or disabled',
  mailbox_suspended: 'The mailbox is suspended',
}

/**
 * Mail a provider accepted that Orbit could not deliver, so the sender got no bounce. No body was kept; add an alias
 * or a catch-all for an address that should exist. Kept for 30 days.
 */
function NotDelivered({ rejected }: { rejected: MailRejectedDelivery[] }) {
  return (
    <SettingsCard
      title="Not delivered"
      description="A provider accepted these messages, but no mailbox could take them, so the sender was not told. Orbit kept no copy. They stay listed for 30 days."
      flush
    >
      <div className="px-2 py-1">
        <Table aria-label="Messages not delivered">
          <TableHeader>
            <TableRow>
              <TableHead>Received</TableHead>
              <TableHead>Sender</TableHead>
              <TableHead>Recipient</TableHead>
              <TableHead>Reason</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rejected.map((entry) => (
              <TableRow key={`${entry.received_at}:${entry.recipient}`}>
                <TableCell className="align-top">{dateTime(entry.received_at)}</TableCell>
                <TableCell className="max-w-56 truncate align-top" title={entry.mail_from ?? undefined}>{entry.mail_from ?? 'Null sender <>'}</TableCell>
                <TableCell className="max-w-56 truncate align-top" title={entry.recipient}>{entry.recipient}</TableCell>
                <TableCell className="max-w-sm align-top whitespace-normal">
                  <span className="text-xs text-muted-foreground">{REASON_TEXTS[entry.reason]} ({entry.provider_name})</span>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </SettingsCard>
  )
}

function FailedReceipts({ total, rows, shown }: { total: number; rows: MailFailedReceipt[]; shown: string }) {
  return (
    <SettingsCard
      title="Failed messages"
      description={`${count(total, 'message', 'messages')} did not reach their mailboxes. Orbit keeps each one; fix the cause, then retry.${shown}`}
      flush
    >
      <div className="px-2 py-1">
        <Table aria-label="Failed messages">
          <TableHeader>
            <TableRow>
              <TableHead>Received</TableHead>
              <TableHead>Sender</TableHead>
              <TableHead>Size</TableHead>
              <TableHead>Error</TableHead>
              <TableHead>Status</TableHead>
              <TableHead><span className="sr-only">Actions</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((receipt) => <ReceiptRow key={receipt.id} receipt={receipt} />)}
          </TableBody>
        </Table>
      </div>
    </SettingsCard>
  )
}

function ReceiptRow({ receipt }: { receipt: MailFailedReceipt }) {
  const retry = useRetryReceipt()
  const sender = receipt.mail_from ?? 'Null sender <>'
  return (
    <TableRow>
      <TableCell className="align-top">{dateTime(receipt.received_at)}</TableCell>
      <TableCell className="max-w-56 truncate align-top" title={sender}>{sender}</TableCell>
      <TableCell className="align-top">{formatSize(receipt.size)}</TableCell>
      <TableCell className="max-w-sm align-top whitespace-normal">
        <span className="text-xs break-words text-muted-foreground">
          {receipt.error ?? (receipt.status === 'stuck' ? 'Waiting for over an hour without a job.' : 'No error was recorded.')}
        </span>
      </TableCell>
      <TableCell className="align-top">
        <div className="flex flex-col items-start gap-0.5">
          <Badge variant={receipt.status === 'failed' ? 'destructive' : 'outline'}>{receipt.status === 'failed' ? 'Failed' : 'Stuck'}</Badge>
          <span className="text-xs text-muted-foreground">{count(receipt.attempts, 'attempt', 'attempts')}</span>
        </div>
      </TableCell>
      <TableCell className="text-right align-top">
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`Retry the message from ${sender}`}
          disabled={retry.isPending}
          onClick={() => retry.mutate(receipt.id, {
            onSuccess: () => toast('Orbit files the message again'),
            onError: (error) => toast.error(mailProblem(error, 'The message could not be retried.')),
          })}
        >
          {retry.isPending ? 'Retrying…' : 'Retry'}
        </Button>
      </TableCell>
    </TableRow>
  )
}
