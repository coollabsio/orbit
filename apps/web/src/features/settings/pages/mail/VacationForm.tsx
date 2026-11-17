import { lazy, Suspense, useId, useRef, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import type { MailVacation } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { useSetVacation } from '@/features/mail/api/mutations'
import { useVacation } from '@/features/mail/api/queries'
import { vacationState } from '@/features/mail/mailStatus'
import { localInputValue, mailProblem } from './mailSettingsLib'

// BlockNote is about 1 MB: loaded when a body field shows
const LazyMailEditor = lazy(() => import('@/features/mail/compose/MailEditor').then((module) => ({ default: module.MailEditor })))
const NO_SNIPPETS: never[] = []
const resolveImage = (url: string) => Promise.resolve(url)

/**
 * A rich text body (signature, snippet, vacation reply) in the mail editor. `initialHtml` is read once: give the field
 * a new `key` to load another body. `onChange` gets the HTML (`''` when blank).
 */
export function MailBodyField({ label, initialHtml, onChange, className }: { label: string; initialHtml: string; onChange: (html: string) => void; className?: string }) {
  return (
    <div role="group" aria-label={label} className={className ?? 'min-h-24 rounded-md border bg-background px-2 py-2'}>
      <Suspense fallback={<div className="flex min-h-20 items-start py-1" aria-busy="true"><Spinner className="size-4 text-muted-foreground" /></div>}>
        <LazyMailEditor
          initialHtml={initialHtml}
          signatureHtml={null}
          snippets={NO_SNIPPETS}
          onChange={(html) => onChange(html())}
          onPasteFiles={() => toast.error('Images cannot be added here.')}
          resolveImage={resolveImage}
          autoFocus={false}
          editable
          className="min-h-20"
        />
      </Suspense>
    </div>
  )
}

/** The vacation reply of a mailbox: the owner's for a personal one, a manager's for a shared one. */
export function VacationForm({ mailboxId, mailboxName }: { mailboxId: string; mailboxName: string }) {
  const vacation = useVacation(mailboxId)
  if (vacation.isPending) return <p role="status" className="m-0 text-sm text-muted-foreground">Loading the vacation reply…</p>
  if (vacation.isError) {
    return (
      <p role="alert" className="m-0 text-sm text-destructive">
        {mailProblem(vacation.error, 'The vacation reply could not be loaded.')}{' '}
        <Button variant="ghost" onClick={() => void vacation.refetch()}>Retry</Button>
      </p>
    )
  }
  // a fresh form for each saved version
  return <VacationFields key={JSON.stringify(vacation.data)} mailboxId={mailboxId} mailboxName={mailboxName} saved={vacation.data} />
}

function VacationFields({ mailboxId, mailboxName, saved }: { mailboxId: string; mailboxName: string; saved: MailVacation }) {
  const ids = useId()
  const save = useSetVacation()
  const [enabled, setEnabled] = useState(saved.enabled)
  const [startsAt, setStartsAt] = useState(() => localInputValue(saved.starts_at, saved.timezone))
  const [endsAt, setEndsAt] = useState(() => localInputValue(saved.ends_at, saved.timezone))
  const [subject, setSubject] = useState(saved.subject)
  const [contactsOnly, setContactsOnly] = useState(saved.contacts_only)
  const body = useRef(saved.body_html)
  const [hasBody, setHasBody] = useState(saved.body_html.trim() !== '')
  const state = vacationState(saved, new Date())

  function submit(event: FormEvent) {
    event.preventDefault()
    if (save.isPending) return
    if (enabled && !hasBody) {
      toast.error('Write the reply before you turn it on.')
      return
    }
    save.mutate(
      {
        mailboxId,
        body: {
          enabled,
          starts_at: startsAt || null,
          ends_at: endsAt || null,
          subject: subject.trim(),
          body_html: body.current,
          contacts_only: contactsOnly,
        },
      },
      {
        onSuccess: (next) => toast(next.enabled ? 'Vacation reply saved' : 'Vacation reply is off'),
        onError: (error) => toast.error(mailProblem(error, 'The vacation reply could not be saved.')),
      },
    )
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={submit}>
      <div className="flex items-center gap-3">
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
          Vacation reply
          <InfoTip text={`Orbit answers mail to ${mailboxName} with this reply, at most once a week for each sender. It never answers lists, bulk mail or automatic messages.`} />
          {state === 'on' ? <span className="text-xs font-normal text-muted-foreground">On now</span> : null}
          {state === 'scheduled' ? <span className="text-xs font-normal text-muted-foreground">Starts later</span> : null}
        </span>
        <Switch aria-label={`Vacation reply for ${mailboxName}`} checked={enabled} onCheckedChange={(checked: boolean) => setEnabled(checked)} />
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field>
          <FieldLabel htmlFor={`${ids}-start`}>
            Starts
            <InfoTip text={`Empty: now. Times are in ${saved.timezone}.`} />
          </FieldLabel>
          <Input id={`${ids}-start`} type="datetime-local" value={startsAt} onChange={(event) => setStartsAt(event.target.value)} />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${ids}-end`}>
            Ends
            <InfoTip text={`Empty: until you turn it off. Times are in ${saved.timezone}.`} />
          </FieldLabel>
          <Input id={`${ids}-end`} type="datetime-local" value={endsAt} min={startsAt || undefined} onChange={(event) => setEndsAt(event.target.value)} />
        </Field>
      </div>
      <Field>
        <FieldLabel htmlFor={`${ids}-subject`}>
          Subject
          <InfoTip text="Empty: “Re:” and the subject of the message." />
        </FieldLabel>
        <Input id={`${ids}-subject`} maxLength={200} autoComplete="off" placeholder="Re: the subject of the message" value={subject} onChange={(event) => setSubject(event.target.value)} />
      </Field>
      <Field>
        <span className="text-sm font-medium">Reply</span>
        <MailBodyField
          label={`Vacation reply of ${mailboxName}`}
          initialHtml={saved.body_html}
          onChange={(html) => { body.current = html; setHasBody(html.trim() !== '') }}
        />
      </Field>
      <div className="flex items-center gap-3">
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
          Only people we wrote to
          <InfoTip text={`Reply only to senders that the members of ${mailboxName} sent mail to before.`} />
        </span>
        <Switch aria-label="Reply only to people we wrote to" checked={contactsOnly} onCheckedChange={(checked: boolean) => setContactsOnly(checked)} />
      </div>
      <div>
        <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save'}</Button>
      </div>
    </form>
  )
}
