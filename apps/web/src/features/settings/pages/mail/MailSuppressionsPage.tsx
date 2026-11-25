import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { ShieldCross } from 'reicon-react'
import type { MailSuppression } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useAddSuppression, useDeleteSuppression } from '@/features/mail/api/mutations'
import { useSuppressions } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCan } from '@/features/workspaces/permissions'
import { useDebouncedValue } from '@/lib/useDebouncedValue'
import { MailManagersOnly } from './MailDomainsPage'
import { mailProblem, since } from './mailSettingsLib'

const REASON_LABELS: Record<string, string> = { hard_bounce: 'Bounced', complaint: 'Complained', manual: 'Added by hand' }

/** Settings → Mail → Suppressions: addresses Orbit no longer sends to, with search, **Add** and **Remove**. */
export function MailSuppressionsPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Suppressions" />
  return <Suppressions />
}

function Suppressions() {
  const [query, setQuery] = useState('')
  const search = useDebouncedValue(query, 250)
  const list = useSuppressions(search)
  const rows = list.data ?? []

  return (
    <SettingsCard
      title="Suppressions"
      description="Addresses that bounced hard, marked your mail as spam or that you added. Orbit does not send to them, so your domains keep a good reputation. Remove an address only when you know it works again."
      actions={
        <Input
          type="search"
          aria-label="Search suppressed addresses"
          className="w-64 max-w-[50vw]"
          placeholder="Search addresses"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      }
      flush
    >
      <AddSuppressionForm />
      {list.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading addresses…</p> : null}
      {list.isError ? (
        <p role="alert" className="m-0 p-4 text-sm text-destructive">
          {mailProblem(list.error, 'The addresses could not be loaded.')}{' '}
          <Button variant="ghost" onClick={() => void list.refetch()}>Retry</Button>
        </p>
      ) : null}
      {list.isSuccess && rows.length === 0 ? (
        <EmptyState
          icon={ShieldCross}
          title={search.trim() ? 'No address matches' : 'No suppressed addresses'}
          description={search.trim() ? undefined : 'An address shows here when it bounces hard, when its owner marks your mail as spam, or when you add it.'}
        />
      ) : null}
      <ul className="m-0 flex list-none flex-col divide-y p-0">
        {rows.map((row) => <SuppressionRow key={row.address} row={row} />)}
      </ul>
      {list.hasNextPage ? (
        <div className="border-t p-2 text-center">
          <Button type="button" variant="ghost" disabled={list.isFetchingNextPage} onClick={() => void list.fetchNextPage()}>
            {list.isFetchingNextPage ? 'Loading…' : 'Show more'}
          </Button>
        </div>
      ) : null}
    </SettingsCard>
  )
}

function AddSuppressionForm() {
  const add = useAddSuppression()
  const [address, setAddress] = useState('')
  const [note, setNote] = useState('')

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = address.trim()
    if (!trimmed || add.isPending) return
    add.mutate({ address: trimmed, detail: note.trim() || undefined }, {
      onSuccess: (entry) => {
        setAddress('')
        setNote('')
        toast(`${entry.address} added`)
      },
    })
  }

  return (
    <div className="border-b">
      <SettingsRow>
        <form className="flex w-full flex-wrap items-center gap-2" onSubmit={submit}>
          <Input
            type="email"
            aria-label="Address to suppress"
            className="w-64 max-w-full"
            placeholder="name@example.com"
            value={address}
            onChange={(event) => setAddress(event.target.value)}
          />
          <Input
            aria-label="Note"
            className="min-w-40 flex-1"
            placeholder="Note (optional)"
            maxLength={500}
            value={note}
            onChange={(event) => setNote(event.target.value)}
          />
          <Button type="submit" disabled={!address.trim() || add.isPending}>Add address</Button>
        </form>
      </SettingsRow>
      {add.isError ? (
        <p role="alert" className="m-0 px-4 pb-3 text-xs text-destructive">{mailProblem(add.error, 'The address could not be added.')}</p>
      ) : null}
    </div>
  )
}

function SuppressionRow({ row }: { row: MailSuppression }) {
  const remove = useDeleteSuppression()

  async function removeRow() {
    const confirmed = await confirmAction({
      title: `Send to ${row.address} again?`,
      description: 'Orbit sends to it again. If it bounces or complains again, it comes back on the list.',
      confirmLabel: 'Remove',
    })
    if (!confirmed) return
    remove.mutate(row.address, {
      onSuccess: () => toast(`${row.address} removed`),
      onError: (error) => toast.error(mailProblem(error, 'The address could not be removed.')),
    })
  }

  return (
    <li className="flex flex-wrap items-start gap-3 px-4 py-3">
      <div className="flex min-w-0 flex-[1_1_16rem] flex-col gap-0.5">
        <div className="flex flex-wrap items-center gap-2">
          <span className="min-w-0 font-mono text-xs break-all">{row.address}</span>
          <Badge variant={row.reason === 'complaint' ? 'destructive' : 'outline'}>{REASON_LABELS[row.reason] ?? row.reason}</Badge>
        </div>
        <span className="text-xs text-muted-foreground">Added {since(row.created_at)}</span>
        {row.detail ? <span className="text-xs break-words text-muted-foreground">{row.detail}</span> : null}
      </div>
      <Button type="button" variant="ghost" size="sm" disabled={remove.isPending} onClick={() => void removeRow()}>Remove</Button>
    </li>
  )
}
