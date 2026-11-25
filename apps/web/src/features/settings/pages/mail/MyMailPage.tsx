import { useId, useRef, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { Add, NoteText } from 'reicon-react'
import type { MailFromAddress, MailSnippet } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { InfoTip } from '@/components/common/InfoTip'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCreateSnippet, useDeleteSnippet, useSaveMailSettings, useSaveSignature, useUpdateSnippet } from '@/features/mail/api/mutations'
import { useFromAddresses, useMailMe, useMailSettingsMe, useSignatures, useSnippets } from '@/features/mail/api/queries'
import { setMailSwipes, SWIPE_ACTIONS, SWIPE_LABELS, useMailSwipes, type SwipeAction, type SwipeSettings } from '@/features/mail/mailSwipe'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { mailProblem, UNDO_SECONDS } from './mailSettingsLib'
import { MailBodyField, VacationForm } from './VacationForm'

/** Settings → Mail → Mail preferences: each member's signatures, undo send delay, swipe actions, snippets and vacation reply. */
export function MyMailPage() {
  return (
    <>
      <SignaturesCard />
      <UndoSendCard />
      <SwipeActionsCard />
      <SnippetsCard />
      <VacationCard />
    </>
  )
}

function SignaturesCard() {
  const addresses = useFromAddresses()
  const list = addresses.data ?? []
  const signatures = useSignatures(list.map((address) => address.address_id))
  const [editing, setEditing] = useState<{ address: MailFromAddress; saved: string } | null>(null)
  const savedFor = (address: MailFromAddress) => signatures.data?.find((signature) => signature.address_id === address.address_id)?.body_html ?? ''
  return (
    <>
      <SettingsCard title="Signatures" description="Added at the end of each new message you write from the address. Each address has its own." flush>
        {addresses.isPending || (list.length > 0 && signatures.isPending) ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading signatures…</p> : null}
        {addresses.isError || signatures.isError ? (
          <p role="alert" className="m-0 p-4 text-sm text-destructive">{mailProblem(addresses.error ?? signatures.error, 'The signatures could not be loaded.')}</p>
        ) : null}
        {addresses.isSuccess && list.length === 0 ? <p className="m-0 p-4 text-sm text-muted-foreground">You have no address to send from yet.</p> : null}
        {signatures.data ? (
          <ul className="m-0 flex list-none flex-col divide-y p-0">
            {list.map((address) => {
              const saved = savedFor(address)
              const preview = plainText(saved)
              return (
                <li key={address.address_id} className="flex items-center gap-3 px-4 py-3">
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[0.8125rem] font-medium text-foreground">
                      {address.display_name} <span className="font-mono text-xs font-normal text-muted-foreground">&lt;{address.address}&gt;</span>
                    </span>
                    <span className="truncate text-xs text-muted-foreground">{preview || 'No signature'}</span>
                  </div>
                  <Button type="button" variant="outline" size="sm" aria-label={`${saved.trim() ? 'Change' : 'Set'} signature for ${address.address}`} onClick={() => setEditing({ address, saved })}>
                    {saved.trim() ? 'Change' : 'Set signature'}
                  </Button>
                </li>
              )
            })}
          </ul>
        ) : null}
      </SettingsCard>
      {editing ? <SignatureForm address={editing.address} saved={editing.saved} onClose={() => setEditing(null)} /> : null}
    </>
  )
}

function SignatureForm({ address, saved, onClose }: { address: MailFromAddress; saved: string; onClose: () => void }) {
  const save = useSaveSignature()
  const html = useRef(saved)

  function submit(event: FormEvent) {
    event.preventDefault()
    if (save.isPending) return
    save.mutate({ addressId: address.address_id, bodyHtml: html.current }, {
      onSuccess: () => { toast(html.current.trim() ? 'Signature saved' : 'Signature removed'); onClose() },
    })
  }

  return (
    <Modal title={`Signature for ${address.address}`} onClose={onClose} dismissible={!save.isPending}>
      <form className="grid gap-4" onSubmit={submit}>
        <MailBodyField label={`Signature for ${address.address}`} placeholder="Write your signature…" initialHtml={saved} onChange={(next) => { html.current = next }} />
        {save.error ? <p role="alert" className="text-sm text-destructive">{mailProblem(save.error, 'The signature could not be saved.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

const UNDO_OPTIONS = UNDO_SECONDS.map((seconds) => ({ value: String(seconds), label: seconds === 0 ? 'Off' : `${seconds} seconds` }))

function UndoSendCard() {
  const settings = useMailSettingsMe()
  const save = useSaveMailSettings()
  return (
    <SettingsCard title="Undo send" flush>
      <SettingsRow className="flex-nowrap">
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
          Time to undo
          <InfoTip text="How long Orbit waits before a message leaves, so you can take it back with Undo." />
        </span>
        <Select
          items={UNDO_OPTIONS}
          value={settings.data ? String(settings.data.undo_send_seconds) : null}
          disabled={!settings.data || save.isPending}
          onValueChange={(value) => save.mutate(Number(value), {
            onSuccess: () => toast('Undo send saved'),
            onError: (error) => toast.error(mailProblem(error, 'The setting could not be saved.')),
          })}
        >
          <SelectTrigger aria-label="Time to undo" className="w-40"><SelectValue placeholder="…" /></SelectTrigger>
          <SelectContent>
            {UNDO_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
          </SelectContent>
        </Select>
      </SettingsRow>
    </SettingsCard>
  )
}

const SWIPE_OPTIONS = SWIPE_ACTIONS.map((value) => ({ value, label: SWIPE_LABELS[value] }))
const SWIPE_SIDES: ReadonlyArray<{ side: keyof SwipeSettings; title: string }> = [
  { side: 'left', title: 'Swipe left' },
  { side: 'right', title: 'Swipe right' },
]

/** The mail list's swipe actions on a touch screen: kept in this browser. */
function SwipeActionsCard() {
  const swipes = useMailSwipes()
  return (
    <SettingsCard title="Swipe actions" description="What a swipe to each side does to a conversation in the mail list, on a touch screen. Saved in this browser." flush>
      <div className="flex flex-col divide-y">
        {SWIPE_SIDES.map(({ side, title }) => (
          <SettingsRow key={side} className="flex-nowrap">
            <span className="min-w-0 flex-1 text-[0.8125rem] font-medium text-foreground">{title}</span>
            <Select items={SWIPE_OPTIONS} value={swipes[side]} onValueChange={(value) => setMailSwipes({ ...swipes, [side]: value as SwipeAction })}>
              <SelectTrigger aria-label={title} className="w-48"><SelectValue /></SelectTrigger>
              <SelectContent>
                {SWIPE_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </SettingsRow>
        ))}
      </div>
    </SettingsCard>
  )
}

function SnippetsCard() {
  const snippets = useSnippets()
  const [editing, setEditing] = useState<MailSnippet | 'new' | null>(null)
  return (
    <>
      <SettingsCard
        title="Snippets"
        description="Text you use often. Type ; in a message to insert one."
        actions={<Button type="button" onClick={() => setEditing('new')}><Add />Add snippet</Button>}
        flush
      >
        {snippets.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading snippets…</p> : null}
        {snippets.isError ? <p role="alert" className="m-0 p-4 text-sm text-destructive">{mailProblem(snippets.error, 'The snippets could not be loaded.')}</p> : null}
        {snippets.data?.length === 0 ? <EmptyState icon={NoteText} size="sm" title="No snippets yet" /> : null}
        <ul className="m-0 flex list-none flex-col divide-y p-0">
          {snippets.data?.map((snippet) => <SnippetRow key={snippet.id} snippet={snippet} onEdit={() => setEditing(snippet)} />)}
        </ul>
      </SettingsCard>
      {editing ? <SnippetForm snippet={editing === 'new' ? undefined : editing} onClose={() => setEditing(null)} /> : null}
    </>
  )
}

/** The text of a snippet's or signature's HTML, for the list. */
const plainText = (html: string) => new DOMParser().parseFromString(html, 'text/html').body.textContent?.trim() ?? ''

function SnippetRow({ snippet, onEdit }: { snippet: MailSnippet; onEdit: () => void }) {
  const remove = useDeleteSnippet()
  async function removeSnippet() {
    const confirmed = await confirmAction({ title: `Delete ${snippet.name}?`, description: 'Messages that use it keep their text.', confirmLabel: 'Delete', danger: true })
    if (!confirmed) return
    remove.mutate(snippet.id, { onError: (error) => toast.error(mailProblem(error, 'The snippet could not be deleted.')) })
  }
  return (
    <li className="flex items-center gap-3 px-4 py-3">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-[0.8125rem] font-medium text-foreground">{snippet.name}</span>
        <span className="truncate text-xs text-muted-foreground">{plainText(snippet.body_html) || 'Empty'}</span>
      </div>
      <Button type="button" variant="ghost" size="sm" onClick={onEdit}>Change</Button>
      <Button type="button" variant="ghost" size="sm" className="text-destructive" disabled={remove.isPending} onClick={() => void removeSnippet()}>Delete</Button>
    </li>
  )
}

function SnippetForm({ snippet, onClose }: { snippet?: MailSnippet; onClose: () => void }) {
  const id = useId()
  const create = useCreateSnippet()
  const update = useUpdateSnippet()
  const [name, setName] = useState(snippet?.name ?? '')
  const body = useRef(snippet?.body_html ?? '')
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || saving) return
    const payload = { name: trimmed, body_html: body.current }
    if (snippet) update.mutate({ snippetId: snippet.id, body: payload }, { onSuccess: onClose })
    else create.mutate(payload, { onSuccess: onClose })
  }

  return (
    <Modal title={snippet ? `Change ${snippet.name}` : 'Add a snippet'} onClose={onClose} dismissible={!saving}>
      <form className="grid gap-4" onSubmit={submit}>
        <Field>
          <FieldLabel htmlFor={`${id}-name`}>Name</FieldLabel>
          <Input id={`${id}-name`} maxLength={80} autoComplete="off" value={name} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Field>
          <span className="text-sm font-medium">Text</span>
          <MailBodyField label="Snippet text" placeholder="Write the snippet…" initialHtml={snippet?.body_html ?? ''} onChange={(html) => { body.current = html }} />
        </Field>
        {error ? <p role="alert" className="text-sm text-destructive">{mailProblem(error, 'The snippet was not saved.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={!name.trim() || saving}>{saving ? 'Saving…' : snippet ? 'Save' : 'Add snippet'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

function VacationCard() {
  const me = useMailMe()
  const personal = me.data?.mailboxes.find((mailbox) => mailbox.kind === 'personal')
  return (
    <SettingsCard title="Vacation reply" description="An automatic answer while you are away, for your personal mailbox. A shared mailbox’s reply is set in Mailboxes.">
      {me.isPending ? <p role="status" className="m-0 text-sm text-muted-foreground">Loading…</p> : null}
      {me.isSuccess && !personal ? <p className="m-0 text-sm text-muted-foreground">You have no personal mailbox.</p> : null}
      {personal ? <VacationForm mailboxId={personal.id} mailboxName={personal.primary_address ?? personal.name} /> : null}
    </SettingsCard>
  )
}
