import { useId, useState } from 'react'
import { toast } from 'sonner'
import type { MailDomain, MailNamePattern } from '@orbit/api/generated/types.gen'
import { Modal } from '@/components/common/Modal'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useBulkPersonal } from '@/features/mail/api/mutations'
import { useBulkPersonalPreview } from '@/features/mail/api/queries'
import { count, mailProblem } from './mailSettingsLib'

const PATTERN_OPTIONS: { value: MailNamePattern; label: string }[] = [
  { value: 'first', label: 'first@ (ann@)' },
  { value: 'first_dot_last', label: 'first.last@ (ann.lee@)' },
  { value: 'flast', label: 'flast@ (alee@)' },
]

/** What the server keeps of a local part: lower-case letters, digits, dots and hyphens, no dot at an end or twice. */
const LOCAL_PART = /^[a-z0-9-]+(\.[a-z0-9-]+)*$/

/**
 * **Create a mailbox for each member**: the admin picks a domain and a pattern, Orbit proposes an ASCII address for
 * each member without a personal mailbox and marks the conflicts, the admin edits any address, then confirms.
 * Members whose address still conflicts get no mailbox.
 */
export function BulkPersonalDialog({ domains, onClose }: { domains: MailDomain[]; onClose: () => void }) {
  const ids = useId()
  const usable = [...domains].sort((a, b) => Number(b.status === 'verified') - Number(a.status === 'verified'))
  const [domainId, setDomainId] = useState(usable[0]?.id ?? '')
  const [pattern, setPattern] = useState<MailNamePattern>('first_dot_last')
  /** The admin's local parts, by membership; cleared when the domain or the pattern changes. */
  const [edits, setEdits] = useState<Record<string, string>>({})
  /** The local parts the server refused at the last confirm, by membership. */
  const [refused, setRefused] = useState<Record<string, string>>({})
  const preview = useBulkPersonalPreview(domainId, pattern)
  const create = useBulkPersonal()
  const domain = domains.find((entry) => entry.id === domainId)
  const domainOptions = usable.map((entry) => ({ value: entry.id, label: entry.status === 'verified' ? entry.name : `${entry.name} (${entry.status})` }))

  const rows = (preview.data ?? []).map((proposal) => {
    const edited = edits[proposal.membership_id]
    const localPart = edited ?? proposal.local_part
    return { ...proposal, localPart, edited: edited !== undefined && edited !== proposal.local_part }
  })
  // like the server: the first member with an address gets it, a later one conflicts
  const seen = new Set<string>()
  const marked = rows.map((row) => {
    const repeated = seen.has(row.localPart)
    seen.add(row.localPart)
    const problem = !LOCAL_PART.test(row.localPart)
      ? 'Not a valid address'
      : repeated
        ? 'Used above'
        : refused[row.membership_id] === row.localPart
          ? 'Taken'
          : !row.edited && row.conflict
            ? 'Taken'
            : null
    return { ...row, problem }
  })
  const ready = marked.filter((row) => row.problem === null).length
  const conflicts = marked.length - ready

  function reset(next: () => void) {
    next()
    setEdits({})
    setRefused({})
  }

  function confirm() {
    if (!domainId || ready === 0 || create.isPending) return
    const overrides = marked.filter((row) => row.edited).map((row) => ({ membership_id: row.membership_id, local_part: row.localPart }))
    create.mutate({ domain_id: domainId, pattern, overrides }, {
      onSuccess: (result) => {
        toast(`${count(result.created.length, 'mailbox', 'mailboxes')} created`)
        if (result.conflicts.length === 0) {
          onClose()
          return
        }
        const asked = new Map(marked.map((row) => [row.membership_id, row.localPart]))
        setRefused(Object.fromEntries(result.conflicts.map((conflict) => [conflict.membership_id, asked.get(conflict.membership_id) ?? conflict.local_part])))
      },
    })
  }

  return (
    <Modal
      title="Create a mailbox for each member"
      description="Each member without a personal mailbox gets one, named after them. Edit any address before you confirm; members marked with a conflict are skipped."
      onClose={onClose}
      className="sm:max-w-3xl"
      dismissible={!create.isPending}
    >
      <div className="grid gap-4">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor={`${ids}-domain`}>Domain</FieldLabel>
            <Select items={domainOptions} value={domainId} onValueChange={(value) => reset(() => setDomainId(value as string))}>
              <SelectTrigger id={`${ids}-domain`}><SelectValue /></SelectTrigger>
              <SelectContent>
                {domainOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor={`${ids}-pattern`}>Pattern</FieldLabel>
            <Select items={PATTERN_OPTIONS} value={pattern} onValueChange={(value) => reset(() => setPattern(value as MailNamePattern))}>
              <SelectTrigger id={`${ids}-pattern`}><SelectValue /></SelectTrigger>
              <SelectContent>
                {PATTERN_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
        </div>

        <div className="max-h-[50vh] overflow-y-auto rounded-lg border">
          {preview.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Making addresses…</p> : null}
          {preview.isError ? <p role="alert" className="m-0 p-4 text-sm text-destructive">{mailProblem(preview.error, 'The addresses could not be made.')}</p> : null}
          {preview.isSuccess && marked.length === 0 ? <p className="m-0 p-4 text-sm text-muted-foreground">Every member has a personal mailbox.</p> : null}
          {marked.length > 0 ? (
            <Table aria-label="Proposed addresses">
              <TableHeader>
                <TableRow>
                  <TableHead>Member</TableHead>
                  <TableHead>Address</TableHead>
                  <TableHead><span className="sr-only">Conflict</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {marked.map((row) => (
                  <TableRow key={row.membership_id} data-conflict={row.problem !== null || undefined}>
                    <TableCell className="max-w-48 truncate" title={row.name}>{row.name}</TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1.5">
                        <Input
                          aria-label={`Address of ${row.name}`}
                          aria-invalid={row.problem !== null || undefined}
                          className="h-7 w-48"
                          autoComplete="off"
                          value={row.localPart}
                          onChange={(event) => setEdits((current) => ({ ...current, [row.membership_id]: event.target.value.trim().toLowerCase() }))}
                        />
                        <span className="text-muted-foreground">@{domain?.name}</span>
                      </div>
                    </TableCell>
                    <TableCell>{row.problem ? <Badge variant="destructive">{row.problem}</Badge> : null}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          ) : null}
        </div>

        {create.isError ? <p role="alert" className="text-sm text-destructive">{mailProblem(create.error, 'The mailboxes were not created.')}</p> : null}
        <DialogFooter>
          <span className="mr-auto self-center text-xs text-muted-foreground">
            {marked.length > 0 ? `${count(ready, 'mailbox', 'mailboxes')} to create${conflicts > 0 ? `, ${count(conflicts, 'conflict', 'conflicts')} skipped` : ''}` : null}
          </span>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="button" disabled={ready === 0 || create.isPending} onClick={confirm}>{create.isPending ? 'Creating…' : `Create ${count(ready, 'mailbox', 'mailboxes')}`}</Button>
        </DialogFooter>
      </div>
    </Modal>
  )
}
