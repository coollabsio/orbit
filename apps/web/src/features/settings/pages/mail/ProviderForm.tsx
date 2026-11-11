import { useId, useState, type FormEvent } from 'react'
import type { MailProviderKind, MailProviderSecretsBody, MailProviderSettings, MailProviderView } from '@orbit/api/generated/types.gen'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCreateProvider, useUpdateProvider } from '@/features/mail/api/mutations'
import type { ProviderScope } from '@/features/mail/api/queries'
import { mailProblem, PROVIDER_LABELS } from './mailSettingsLib'

const KIND_OPTIONS = (Object.keys(PROVIDER_LABELS) as MailProviderKind[]).map((kind) => ({ value: kind, label: PROVIDER_LABELS[kind] }))

const MAILGUN_REGIONS = [
  { value: 'us', label: 'US' },
  { value: 'eu', label: 'EU' },
]

/** The secrets each kind needs from its provider; Orbit makes Cloudflare's and Postmark's. */
const SECRET_FIELDS: Record<MailProviderKind, { name: keyof MailProviderSecretsBody; label: string; hint?: string }[]> = {
  cloudflare: [],
  postmark: [],
  mailgun: [{ name: 'signing_key', label: 'HTTP webhook signing key' }],
  resend: [
    { name: 'api_key', label: 'API key', hint: 'A key that can read received emails.' },
    { name: 'webhook_secret', label: 'Webhook signing secret', hint: 'whsec_…' },
  ],
  ses: [
    { name: 'access_key_id', label: 'Access key ID', hint: 'An IAM user with s3:GetObject and s3:DeleteObject on the bucket.' },
    { name: 'secret_access_key', label: 'Secret access key' },
  ],
}

const hostsText = (hosts: string[]) => hosts.join(', ')
const hostsList = (text: string) => text.split(/[\s,]+/).map((host) => host.trim()).filter(Boolean)

/** Add a provider (kind, name, its secrets and settings), or change one: secrets left empty stay as they are. */
export function ProviderForm({
  scope,
  provider,
  onClose,
  onCreated,
}: {
  scope: ProviderScope
  provider?: MailProviderView
  onClose: () => void
  onCreated?: (created: MailProviderView) => void
}) {
  const ids = useId()
  const create = useCreateProvider(scope)
  const update = useUpdateProvider(scope)
  const [kind, setKind] = useState<MailProviderKind>(provider?.kind ?? 'cloudflare')
  const [name, setName] = useState(provider?.name ?? '')
  const [secrets, setSecrets] = useState<MailProviderSecretsBody>({})
  const [settings, setSettings] = useState<MailProviderSettings>(provider?.settings ?? { mx_hosts: [] })
  const [mx, setMx] = useState(hostsText(provider?.settings.mx_hosts ?? []))
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error
  const fields = SECRET_FIELDS[kind]
  const secretsGiven = fields.every((field) => (secrets[field.name] ?? '').trim() !== '')
  const secretsTouched = fields.some((field) => (secrets[field.name] ?? '').trim() !== '')
  const setting = (key: 'region' | 'bucket' | 'prefix' | 'topic_arn', value: string) =>
    setSettings((current) => ({ ...current, [key]: value.trim() === '' ? null : value }))

  function submit(event: FormEvent) {
    event.preventDefault()
    if (saving) return
    const body = { name: name.trim() || PROVIDER_LABELS[kind], settings: { ...settings, mx_hosts: hostsList(mx) } }
    if (provider) {
      update.mutate(
        { providerId: provider.id, body: { ...body, secrets: secretsTouched ? secrets : null } },
        { onSuccess: onClose },
      )
      return
    }
    create.mutate(
      { kind, ...body, secrets: fields.length > 0 ? secrets : null },
      { onSuccess: (created) => { onCreated?.(created); onClose() } },
    )
  }

  const ready = provider ? !secretsTouched || secretsGiven : secretsGiven
  return (
    <Modal
      title={provider ? `Change ${provider.name}` : 'Add a mail provider'}
      description="The provider receives the domain's mail and sends each message to Orbit over HTTPS: this server needs no open port 25."
      onClose={onClose}
      dismissible={!saving}
    >
      <form className="grid gap-4" onSubmit={submit}>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor={`${ids}-kind`}>Provider</FieldLabel>
            <Select items={KIND_OPTIONS} value={kind} disabled={Boolean(provider)} onValueChange={(value) => { setKind(value as MailProviderKind); setSecrets({}) }}>
              <SelectTrigger id={`${ids}-kind`}><SelectValue /></SelectTrigger>
              <SelectContent>
                {KIND_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor={`${ids}-name`}>Name</FieldLabel>
            <Input id={`${ids}-name`} autoComplete="off" placeholder={PROVIDER_LABELS[kind]} value={name} onChange={(event) => setName(event.target.value)} />
          </Field>
        </div>
        {fields.length === 0 && !provider ? (
          <p className="m-0 text-[0.8125rem] text-muted-foreground">Orbit makes the secret for this provider and shows it once, after you add it.</p>
        ) : null}
        {fields.map((field) => (
          <Field key={field.name}>
            <FieldLabel htmlFor={`${ids}-${field.name}`}>{field.label}</FieldLabel>
            <Input
              id={`${ids}-${field.name}`}
              type="password"
              autoComplete="off"
              placeholder={provider?.secrets_set.includes(field.name) ? 'Set. Leave empty to keep it.' : undefined}
              value={secrets[field.name] ?? ''}
              onChange={(event) => setSecrets((current) => ({ ...current, [field.name]: event.target.value }))}
            />
            {field.hint ? <FieldDescription>{field.hint}</FieldDescription> : null}
          </Field>
        ))}
        {kind === 'mailgun' ? (
          <Field>
            <FieldLabel htmlFor={`${ids}-region`}>Region</FieldLabel>
            <Select items={MAILGUN_REGIONS} value={settings.region === 'eu' ? 'eu' : 'us'} onValueChange={(value) => setting('region', value === 'eu' ? 'eu' : '')}>
              <SelectTrigger id={`${ids}-region`} className="w-40"><SelectValue /></SelectTrigger>
              <SelectContent>
                {MAILGUN_REGIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
        ) : null}
        {kind === 'ses' ? (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor={`${ids}-ses-region`}>AWS region</FieldLabel>
              <Input id={`${ids}-ses-region`} autoComplete="off" placeholder="us-east-1" value={settings.region ?? ''} onChange={(event) => setting('region', event.target.value)} />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${ids}-bucket`}>S3 bucket</FieldLabel>
              <Input id={`${ids}-bucket`} autoComplete="off" value={settings.bucket ?? ''} onChange={(event) => setting('bucket', event.target.value)} />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${ids}-prefix`}>Key prefix</FieldLabel>
              <Input id={`${ids}-prefix`} autoComplete="off" placeholder="Optional" value={settings.prefix ?? ''} onChange={(event) => setting('prefix', event.target.value)} />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${ids}-topic`}>SNS topic ARN</FieldLabel>
              <Input id={`${ids}-topic`} autoComplete="off" placeholder="arn:aws:sns:…" value={settings.topic_arn ?? ''} onChange={(event) => setting('topic_arn', event.target.value)} />
            </Field>
          </div>
        ) : null}
        <Field>
          <FieldLabel htmlFor={`${ids}-mx`}>MX hosts</FieldLabel>
          <Input id={`${ids}-mx`} autoComplete="off" placeholder={kind === 'resend' ? 'The MX host that Resend shows' : 'Empty: the provider’s usual hosts'} value={mx} onChange={(event) => setMx(event.target.value)} />
          <FieldDescription>Domains routed to this provider must point their MX to one of these. Separate them with commas.</FieldDescription>
        </Field>
        {error ? <p role="alert" className="text-sm text-destructive">{mailProblem(error, 'The provider was not saved.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={saving || !ready}>{saving ? 'Saving…' : provider ? 'Save' : 'Add provider'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
