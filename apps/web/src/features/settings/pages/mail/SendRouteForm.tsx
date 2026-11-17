import { useId, useState, type FormEvent } from 'react'
import { Danger } from 'reicon-react'
import type { MailProviderView, MailSendRouteView, RouteSettings, SmtpSecurity } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { Modal } from '@/components/common/Modal'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCreateProviderSendRoute, useCreateSendRoute, useUpdateSendRoute } from '@/features/mail/api/mutations'
import type { ProviderScope } from '@/features/mail/api/queries'
import { RequiredMark } from '@/features/settings/components/SettingsParts'
import { CopyButton } from './MailDomainRecords'
import {
  CLOUDFLARE_EVENTS_WORKER,
  defaultMaxBytes,
  eventsUrl,
  eventsUrlWithLogin,
  generatedSecrets,
  hasEvents,
  isSendRouteKind,
  mailProblem,
  megabytes,
  providerRouteFields,
  ROUTE_LABELS,
  routeFields,
  routeKindOfProvider,
  routeSetupSteps,
  SECRET_LABELS,
  SEND_ROUTE_KINDS,
  type FieldSpec,
  type SendRouteKind,
} from './mailSettingsLib'

/** Random hex of `size` bytes, for a secret Orbit makes for a route: the provider sends it back with each event. */
function randomHex(size: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(size))
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

/** A value for each secret Orbit makes: a short user name, else 32 random bytes. */
const newSecret = (name: string) => (name === 'webhook_user' ? `orbit-${randomHex(6)}` : randomHex(32))

/** A new route and the secrets Orbit made for it (shown once). */
export interface CreatedRoute {
  route: MailSendRouteView
  generated: Record<string, string>
}

type Values = Record<string, string>

/** The form's text of each setting of a saved route. */
function settingValues(settings: RouteSettings | null | undefined): Values {
  const values: Values = {}
  for (const [name, value] of Object.entries(settings ?? {})) {
    if (value !== null && value !== undefined && name !== 'max_message_bytes') values[name] = String(value)
  }
  return values
}

/** The settings the server gets: `base` (what the form does not show) with the form's values over it. */
function settingsBody(fields: FieldSpec[], values: Values, maxBytes: string, base: RouteSettings | null | undefined): RouteSettings {
  const settings: Record<string, unknown> = { ...(base ?? {}) }
  for (const field of fields.filter((spec) => spec.group === 'settings')) {
    const value = (values[field.name] ?? '').trim()
    if (field.input === 'number') settings[field.name] = value === '' ? null : Number(value)
    else if (field.name === 'security') settings.security = (value || 'starttls') as SmtpSecurity
    else settings[field.name] = value === '' ? null : value
  }
  const bytes = maxBytes.trim()
  settings.max_message_bytes = bytes === '' ? 0 : Math.round(Number(bytes) * 1_000_000)
  return settings as RouteSettings
}

function secretsBody(fields: FieldSpec[], values: Values): Record<string, string> {
  const secrets: Record<string, string> = {}
  for (const field of fields.filter((spec) => spec.group === 'secrets')) {
    const value = (values[field.name] ?? '').trim()
    if (value !== '') secrets[field.name] = value
  }
  return secrets
}

/** Whether the form can be sent: required settings set, required secrets set on a new route, an SMTP login whole. */
function ready(fields: FieldSpec[], settings: Values, secrets: Values, editing: boolean): boolean {
  const filled = (values: Values, name: string) => (values[name] ?? '').trim() !== ''
  for (const field of fields.filter((spec) => spec.required)) {
    const values = field.group === 'settings' ? settings : secrets
    if (field.group === 'secrets' && editing) continue
    if (!filled(values, field.name)) return false
  }
  const login = ['username', 'password'].filter((name) => fields.some((field) => field.name === name))
  if (login.length === 2 && !editing && filled(secrets, 'username') !== filled(secrets, 'password')) return false
  return true
}

/** The inputs of `fields`; secret inputs stay empty, and a set secret says so. */
function RouteFields({
  ids,
  fields,
  settings,
  secrets,
  secretsSet,
  onSetting,
  onSecret,
}: {
  ids: string
  fields: FieldSpec[]
  settings: Values
  secrets: Values
  secretsSet: string[]
  onSetting: (name: string, value: string) => void
  onSecret: (name: string, value: string) => void
}) {
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {fields.map((field) => {
        const id = `${ids}-${field.group}-${field.name}`
        const values = field.group === 'settings' ? settings : secrets
        const set = field.group === 'settings' ? onSetting : onSecret
        const label = (
          <FieldLabel htmlFor={id}>
            {field.label}
            {field.required && !(field.group === 'secrets' && secretsSet.includes(field.name)) ? <RequiredMark /> : null}
            {field.help ? <InfoTip text={field.help} /> : null}
          </FieldLabel>
        )
        if (field.input === 'select' && field.options) {
          const options = field.options
          return (
            <Field key={id}>
              {label}
              <Select items={options} value={values[field.name] || options[0]!.value} onValueChange={(value) => set(field.name, value as string)}>
                <SelectTrigger id={id}><SelectValue /></SelectTrigger>
                <SelectContent>
                  {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                </SelectContent>
              </Select>
            </Field>
          )
        }
        return (
          <Field key={id}>
            {label}
            <Input
              id={id}
              type={field.input}
              inputMode={field.input === 'number' ? 'numeric' : undefined}
              autoComplete="off"
              placeholder={field.group === 'secrets' && secretsSet.includes(field.name) ? 'Set. Leave empty to keep it.' : field.placeholder}
              value={values[field.name] ?? ''}
              onChange={(event) => set(field.name, event.target.value)}
            />
          </Field>
        )
      })}
    </div>
  )
}

function MaxBytesField({ id, kind, value, onChange }: { id: string; kind: SendRouteKind; value: string; onChange: (value: string) => void }) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>
        Largest message (MB)
        <InfoTip text={`Messages are refused above this size, as sent (attachments grow by a third). Empty: ${megabytes(defaultMaxBytes(kind))}, the most this kind takes.`} />
      </FieldLabel>
      <Input id={id} className="w-40" inputMode="decimal" autoComplete="off" placeholder={String(defaultMaxBytes(kind) / 1_000_000)} value={value} onChange={(event) => onChange(event.target.value)} />
    </Field>
  )
}

/** Add a send route (kind, name, its secrets and settings), or change one: secrets left empty stay as they are. */
export function SendRouteForm({
  scope,
  route,
  directAvailable,
  onClose,
  onCreated,
}: {
  scope: ProviderScope
  route?: MailSendRouteView
  directAvailable: boolean
  onClose: () => void
  onCreated?: (created: CreatedRoute) => void
}) {
  const ids = useId()
  const create = useCreateSendRoute(scope)
  const update = useUpdateSendRoute(scope)
  const firstKind: SendRouteKind = route && isSendRouteKind(route.kind) ? route.kind : directAvailable ? 'direct' : 'smtp'
  const [kind, setKind] = useState<SendRouteKind>(firstKind)
  const [name, setName] = useState(route?.name ?? '')
  const [settings, setSettings] = useState<Values>(() => settingValues(route?.settings))
  const [secrets, setSecrets] = useState<Values>({})
  const [maxBytes, setMaxBytes] = useState(() => {
    const bytes = route?.settings?.max_message_bytes ?? 0
    return bytes > 0 && bytes !== defaultMaxBytes(firstKind) ? String(bytes / 1_000_000) : ''
  })
  const fields = routeFields(kind).filter((field) => !field.instanceOnly || scope === 'instance')
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error
  const kindOptions = SEND_ROUTE_KINDS.map((value) => ({
    value,
    label: value === 'direct' && !directAvailable ? `${ROUTE_LABELS[value]} (needs ORBIT_MAIL_HOSTNAME)` : ROUTE_LABELS[value],
  }))

  function submit(event: FormEvent) {
    event.preventDefault()
    if (saving || !ready(fields, settings, secrets, Boolean(route))) return
    const body = { name: name.trim() || ROUTE_LABELS[kind], settings: settingsBody(fields, settings, maxBytes, route?.settings) }
    const given = secretsBody(fields, secrets)
    if (route) {
      update.mutate({ routeId: route.id, body: { ...body, secrets: Object.keys(given).length > 0 ? given : null } }, { onSuccess: onClose })
      return
    }
    const generated = Object.fromEntries(generatedSecrets(kind).map((secret) => [secret, newSecret(secret)]))
    create.mutate(
      { kind, ...body, secrets: { ...given, ...generated } },
      { onSuccess: (created) => { onCreated?.({ route: created, generated }); onClose() } },
    )
  }

  return (
    <Modal
      title={route ? `Change ${route.name}` : 'Add a send route'}
      description="How Orbit sends the mail of the domains that use this route."
      onClose={onClose}
      dismissible={!saving}
    >
      <form className="grid gap-4" onSubmit={submit}>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor={`${ids}-kind`}>Kind</FieldLabel>
            <Select
              items={kindOptions}
              value={kind}
              disabled={Boolean(route)}
              onValueChange={(value) => { setKind(value as SendRouteKind); setSecrets({}); setSettings({}); setMaxBytes('') }}
            >
              <SelectTrigger id={`${ids}-kind`}><SelectValue /></SelectTrigger>
              <SelectContent>
                {kindOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value} disabled={option.value === 'direct' && !directAvailable}>{option.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor={`${ids}-name`}>Name</FieldLabel>
            <Input id={`${ids}-name`} autoComplete="off" maxLength={80} placeholder={ROUTE_LABELS[kind]} value={name} onChange={(event) => setName(event.target.value)} />
          </Field>
        </div>
        {kind === 'direct' ? (
          <p className="m-0 text-[0.8125rem] text-muted-foreground">Orbit delivers to each recipient’s mail server on port 25. The server must be allowed to send on port 25, with a PTR record for its address.</p>
        ) : null}
        {!route && generatedSecrets(kind).length > 0 ? (
          <p className="m-0 text-[0.8125rem] text-muted-foreground">Orbit makes the {kind === 'postmark' ? 'webhook user name and password' : 'event secret'} and shows {kind === 'postmark' ? 'them' : 'it'} once, after you add the route.</p>
        ) : null}
        <RouteFields
          ids={ids}
          fields={fields}
          settings={settings}
          secrets={secrets}
          secretsSet={route?.secrets_set ?? []}
          onSetting={(field, value) => setSettings((current) => ({ ...current, [field]: value }))}
          onSecret={(field, value) => setSecrets((current) => ({ ...current, [field]: value }))}
        />
        <MaxBytesField id={`${ids}-max`} kind={kind} value={maxBytes} onChange={setMaxBytes} />
        {error ? <p role="alert" className="text-sm text-destructive">{mailProblem(error, 'The route was not saved.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={saving || !ready(fields, settings, secrets, Boolean(route))}>{saving ? 'Saving…' : route ? 'Save' : 'Add route'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}

/** **Also send through this provider**: a route with the provider's account; the form asks only for what it lacks. */
export function ProviderSendRouteForm({
  scope,
  provider,
  onClose,
  onCreated,
}: {
  scope: ProviderScope
  provider: MailProviderView
  onClose: () => void
  onCreated?: (created: CreatedRoute) => void
}) {
  const ids = useId()
  const create = useCreateProviderSendRoute(scope)
  const kind = routeKindOfProvider(provider.kind)
  const fields = providerRouteFields(kind).filter((field) => !field.instanceOnly || scope === 'instance')
  const [settings, setSettings] = useState<Values>({})
  const [secrets, setSecrets] = useState<Values>({})
  const [maxBytes, setMaxBytes] = useState('')

  function submit(event: FormEvent) {
    event.preventDefault()
    if (create.isPending || !ready(fields, settings, secrets, false)) return
    create.mutate(
      { providerId: provider.id, body: { secrets: secretsBody(fields, secrets), settings: settingsBody(fields, settings, maxBytes, null) } },
      { onSuccess: (route) => { onCreated?.({ route, generated: {} }); onClose() } },
    )
  }

  return (
    <Modal
      title={`Also send through ${provider.name}`}
      description={`A send route with your ${ROUTE_LABELS[kind]} account. Orbit reuses what the provider already has.`}
      onClose={onClose}
      dismissible={!create.isPending}
    >
      <form className="grid gap-4" onSubmit={submit}>
        {fields.length === 0 ? <p className="m-0 text-[0.8125rem] text-muted-foreground">Nothing else is needed.</p> : null}
        <RouteFields
          ids={ids}
          fields={fields}
          settings={settings}
          secrets={secrets}
          secretsSet={[]}
          onSetting={(field, value) => setSettings((current) => ({ ...current, [field]: value }))}
          onSecret={(field, value) => setSecrets((current) => ({ ...current, [field]: value }))}
        />
        <MaxBytesField id={`${ids}-max`} kind={kind} value={maxBytes} onChange={setMaxBytes} />
        {create.error ? <p role="alert" className="text-sm text-destructive">{mailProblem(create.error, 'The route was not added.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={create.isPending || !ready(fields, settings, secrets, false)}>{create.isPending ? 'Adding…' : 'Add route'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
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
 * What to set up at the provider for a route: the events URL, the steps and (Cloudflare) the queue Worker. `generated`
 * holds the secrets Orbit made, which only the moment of creation has.
 */
export function RouteSetup({ route, generated }: { route: MailSendRouteView; generated?: Record<string, string> }) {
  if (!isSendRouteKind(route.kind)) return null
  const kind = route.kind
  const secrets = Object.entries(generated ?? {})
  const url = eventsUrl(window.location.origin, kind, route.id)
  const login = generated?.webhook_user && generated.webhook_password ? { user: generated.webhook_user, password: generated.webhook_password } : null
  return (
    <div className="flex flex-col gap-4">
      {secrets.length > 0 ? (
        <Alert>
          <Danger />
          <AlertTitle>Copy these now. Orbit does not show them again.</AlertTitle>
          <AlertDescription>If you lose them, remove the route and add it again.</AlertDescription>
        </Alert>
      ) : null}
      {hasEvents(kind) ? <CopyRow label="Events URL" value={eventsUrlWithLogin(url, login)} /> : null}
      {kind === 'postmark' && !login ? (
        <p className="m-0 text-xs text-muted-foreground">
          {route.provider_id
            ? 'Put the user name and password of the provider’s inbound webhook in this URL (https://user:password@…).'
            : 'Put the webhook user name and password Orbit showed when you added the route in this URL (https://user:password@…).'}
        </p>
      ) : null}
      {secrets.map(([name, value]) => <CopyRow key={name} label={SECRET_LABELS[name] ?? name} value={value} />)}
      <ol className="m-0 flex list-decimal flex-col gap-1.5 pl-5 text-[0.8125rem]">
        {routeSetupSteps(kind).map((step) => <li key={step}>{step}</li>)}
      </ol>
      {kind === 'cloudflare' ? (
        <div className="flex flex-col gap-1">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-medium text-muted-foreground">Queue Worker script</span>
            <CopyButton text={CLOUDFLARE_EVENTS_WORKER} label="Script" />
          </div>
          <pre className="m-0 max-h-72 overflow-auto rounded-md border bg-muted/40 p-2 font-mono text-xs">{CLOUDFLARE_EVENTS_WORKER}</pre>
        </div>
      ) : null}
    </div>
  )
}
