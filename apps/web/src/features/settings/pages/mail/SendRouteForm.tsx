import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { Danger } from 'reicon-react'
import type { MailProviderView, MailSendRouteView } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
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
import { ProviderLinks } from './ProviderLinks'
import {
  CLOUDFLARE_EVENTS_WORKER,
  defaultMaxBytes,
  eventsUrlWithLogin,
  generatedSecrets,
  initialSettingValues,
  isSendRouteKind,
  mailProblem,
  maxBytesValid,
  megabytes,
  ROUTE_LABELS,
  routeFormFields,
  routeFormReady,
  routeKindOfProvider,
  routeSetupSteps,
  SECRET_LABELS,
  secretsBody,
  SEND_ROUTE_KINDS,
  settingsBody,
  type FieldSpec,
  type FormValues,
  type SendRouteKind,
} from './mailSettingsLib'

/** The help of the Kind field for the chosen kind. */
function kindHelp(kind: SendRouteKind): string | null {
  switch (kind) {
    case 'direct':
      return 'Orbit delivers to each recipient’s mail server on port 25. This server must be allowed to send on port 25, and the PTR record of its address must name the mail hostname, or receivers mark the mail as spam. The Health button on Admin → Mail checks it.'
    default: {
      const made = generatedSecrets(kind).map((name) => (SECRET_LABELS[name] ?? name).toLowerCase())
      if (made.length === 0) return null
      return `Orbit makes the ${made.join(' and ')} and shows ${made.length > 1 ? 'them' : 'it'} once, after you add the route.`
    }
  }
}

/** The inputs of `fields`; secret inputs stay empty, and a set secret says so. */
function RouteFields({
  kind,
  ids,
  fields,
  settings,
  secrets,
  secretsSet,
  onSetting,
  onSecret,
}: {
  kind: SendRouteKind
  ids: string
  fields: FieldSpec[]
  settings: FormValues
  secrets: FormValues
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
              <Select items={options} value={values[field.name] || field.defaultValue || null} onValueChange={(value) => set(field.name, value as string)}>
                <SelectTrigger id={id}><SelectValue /></SelectTrigger>
                <SelectContent>
                  {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                </SelectContent>
              </Select>
              <ProviderLinks kind={kind} purpose="send" field={field.name} />
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
            <ProviderLinks kind={kind} purpose="send" field={field.name} />
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
      <Input
        id={id}
        className="w-40"
        inputMode="decimal"
        autoComplete="off"
        aria-invalid={!maxBytesValid(value) || undefined}
        placeholder={String(defaultMaxBytes(kind) / 1_000_000)}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
      {!maxBytesValid(value) ? <p className="m-0 text-xs text-destructive">Enter a size in MB above 0, or leave it empty.</p> : null}
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
  /** The new route, with the secrets the server made (`generated`, shown once). */
  onCreated?: (created: MailSendRouteView) => void
}) {
  const ids = useId()
  const create = useCreateSendRoute(scope)
  const update = useUpdateSendRoute(scope)
  const firstKind: SendRouteKind = route && isSendRouteKind(route.kind) ? route.kind : directAvailable ? 'direct' : 'smtp'
  const [kind, setKind] = useState<SendRouteKind>(firstKind)
  const fields = routeFormFields(kind, scope)
  const [name, setName] = useState(route?.name ?? ROUTE_LABELS[firstKind])
  const [settings, setSettings] = useState<FormValues>(() => initialSettingValues(fields, route?.settings))
  const [secrets, setSecrets] = useState<FormValues>({})
  const [maxBytes, setMaxBytes] = useState(() => {
    const bytes = route?.settings?.max_message_bytes ?? 0
    return bytes > 0 && bytes !== defaultMaxBytes(firstKind) ? String(bytes / 1_000_000) : ''
  })
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error
  const ready = routeFormReady(fields, { settings, secrets, maxBytes, editing: Boolean(route) })
  const hasLogin = kind === 'smtp' && (route?.secrets_set.some((name) => name === 'username' || name === 'password') ?? false)
  const help = kindHelp(kind)
  const kindOptions = SEND_ROUTE_KINDS.map((value) => ({
    value,
    label: value === 'direct' && !directAvailable ? `${ROUTE_LABELS[value]} (needs ORBIT_MAIL_HOSTNAME)` : ROUTE_LABELS[value],
  }))

  function chooseKind(next: SendRouteKind) {
    setKind(next)
    // keep a name the user typed; replace the previous kind's prefilled one
    setName((current) => (current.trim() === '' || current === ROUTE_LABELS[kind] ? ROUTE_LABELS[next] : current))
    setSecrets({})
    setSettings(initialSettingValues(routeFormFields(next, scope), null))
    setMaxBytes('')
  }

  function submit(event: FormEvent) {
    event.preventDefault()
    if (saving || !ready) return
    const body = { name: name.trim() || ROUTE_LABELS[kind], settings: settingsBody(fields, settings, maxBytes, route?.settings) }
    const given = secretsBody(fields, secrets)
    if (route) {
      update.mutate({ routeId: route.id, body: { ...body, secrets: Object.keys(given).length > 0 ? given : null } }, { onSuccess: onClose })
      return
    }
    // the server makes the webhook secrets it checks (Postmark, Cloudflare) and answers with them once
    create.mutate({ kind, ...body, secrets: given }, { onSuccess: (created) => { onCreated?.(created); onClose() } })
  }

  async function removeLogin() {
    if (!route) return
    const confirmed = await confirmAction({
      title: `Remove the login of ${route.name}?`,
      description: 'Orbit connects to the relay without a user name and password.',
      confirmLabel: 'Remove login',
      danger: true,
    })
    if (!confirmed) return
    update.mutate({ routeId: route.id, body: { clear_login: true } }, {
      // a failure shows under the form, as for Save
      onSuccess: () => { toast('Login removed'); onClose() },
    })
  }

  return (
    <Modal
      title={route ? `Change ${route.name}` : 'Add an outgoing provider'}
      description="Outgoing mail: how Orbit sends the mail of the domains that use it."
      onClose={onClose}
      dismissible={!saving}
    >
      <form className="grid gap-4" onSubmit={submit}>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor={`${ids}-kind`}>
              Kind
              {help && !route ? <InfoTip text={help} /> : null}
            </FieldLabel>
            <Select items={kindOptions} value={kind} disabled={Boolean(route)} onValueChange={(value) => chooseKind(value as SendRouteKind)}>
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
        <ProviderLinks kind={kind} purpose="send" />
        <RouteFields
          kind={kind}
          ids={ids}
          fields={fields}
          settings={settings}
          secrets={secrets}
          secretsSet={route?.secrets_set ?? []}
          onSetting={(field, value) => setSettings((current) => ({ ...current, [field]: value }))}
          onSecret={(field, value) => setSecrets((current) => ({ ...current, [field]: value }))}
        />
        {hasLogin ? (
          <div>
            <Button type="button" variant="ghost" size="sm" className="text-destructive" disabled={saving} onClick={() => void removeLogin()}>Remove login</Button>
          </div>
        ) : null}
        <MaxBytesField id={`${ids}-max`} kind={kind} value={maxBytes} onChange={setMaxBytes} />
        {error ? <p role="alert" className="text-sm text-destructive">{mailProblem(error, 'The route was not saved.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={saving || !ready}>{saving ? 'Saving…' : route ? 'Save' : 'Add provider'}</Button>
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
  onCreated?: (created: MailSendRouteView) => void
}) {
  const ids = useId()
  const create = useCreateProviderSendRoute(scope)
  const kind = routeKindOfProvider(provider.kind)
  const fields = routeFormFields(kind, scope, true)
  const [settings, setSettings] = useState<FormValues>(() => initialSettingValues(fields, null))
  const [secrets, setSecrets] = useState<FormValues>({})
  const [maxBytes, setMaxBytes] = useState('')
  const ready = routeFormReady(fields, { settings, secrets, maxBytes, editing: false })

  function submit(event: FormEvent) {
    event.preventDefault()
    if (create.isPending || !ready) return
    create.mutate(
      { providerId: provider.id, body: { secrets: secretsBody(fields, secrets), settings: settingsBody(fields, settings, maxBytes, null) } },
      { onSuccess: (route) => { onCreated?.(route); onClose() } },
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
        <RouteFields
          kind={kind}
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
          <Button type="submit" disabled={create.isPending || !ready}>{create.isPending ? 'Adding…' : 'Add route'}</Button>
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
 * What to set up at the provider for a route: the events URL (from the server), the steps and (Cloudflare) the queue
 * Worker. The secrets the server made (`route.generated`) are only in the answer that created the route.
 */
export function RouteSetup({ route }: { route: MailSendRouteView }) {
  if (!isSendRouteKind(route.kind)) return null
  const kind = route.kind
  const secrets = Object.entries(route.generated ?? {})
  const generated = route.generated ?? {}
  const login = generated.webhook_user && generated.webhook_password ? { user: generated.webhook_user, password: generated.webhook_password } : null
  const fromProvider = route.event_secret_from_provider
  return (
    <div className="flex flex-col gap-4">
      {secrets.length > 0 ? (
        <Alert>
          <Danger />
          <AlertTitle>Copy these now. Orbit does not show them again.</AlertTitle>
          <AlertDescription>If you lose them, remove the route and add it again.</AlertDescription>
        </Alert>
      ) : null}
      {route.events_url ? <CopyRow label="Events URL" value={eventsUrlWithLogin(route.events_url, login)} /> : null}
      {kind === 'postmark' && !login ? (
        <p className="m-0 text-xs text-muted-foreground">
          {route.provider_id
            ? 'Put the user name and password of the provider’s inbound webhook in this URL (https://user:password@…).'
            : 'Put the webhook user name and password Orbit showed when you added the route in this URL (https://user:password@…).'}
        </p>
      ) : null}
      {secrets.map(([name, value]) => <CopyRow key={name} label={SECRET_LABELS[name] ?? name} value={value} />)}
      <ol className="m-0 flex list-decimal flex-col gap-1.5 pl-5 text-[0.8125rem]">
        {routeSetupSteps(kind, fromProvider).map((step) => <li key={step}>{step}</li>)}
      </ol>
      {kind === 'cloudflare' && !fromProvider ? (
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
