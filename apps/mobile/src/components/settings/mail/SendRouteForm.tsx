/**
 * The web's `SendRouteForm.tsx`: add or change a send route (Settings → Mail → Providers, Admin → Mail), add one with a
 * provider's account, and what to set up at the provider once it exists. `scope` picks the root user's routes
 * (`instance`) or the workspace's.
 */
import * as Clipboard from 'expo-clipboard'
import { getRandomBytes } from 'expo-crypto'
import { type ReactNode, useState } from 'react'
import { ScrollView, View } from 'react-native'

import type { MailProviderView } from '@orbit/api/generated/types.gen'
import type { MailSendRouteView, ProviderScope } from '../../../data/mail-sending-api'
import {
  CLOUDFLARE_EVENTS_WORKER,
  defaultMaxBytes,
  eventsUrlInForm,
  eventsUrlWithLogin,
  type FieldSpec,
  type FormValues,
  generatedSecrets,
  initialSettingValues,
  isSendRouteKind,
  mailProblem,
  maxBytesValid,
  megabytes,
  missingEventSecret,
  ROUTE_LABELS,
  routeFormFields,
  routeFormReady,
  routeKindOfProvider,
  routeSetupSteps,
  SECRET_LABELS,
  secretsBody,
  SEND_ROUTE_KINDS,
  type SendRouteKind,
  settingsBody,
  uuidV7,
} from '../../../data/mail-settings'
import { useAuthOptions } from '../../../hooks/account'
import { useSendRouteWrites } from '../../../hooks/mail-sending'
import { Button, Input, LabelTip, Text } from '../../../ui'
import { Dialog } from '../Dialog'
import { FieldLabel, Note, Select, SmallOutlineButton } from '../Shell'
import { ProviderLinks } from './ProviderLinks'

/** A new route id. Hermes may lack `crypto.getRandomValues`, so the random bytes come from expo-crypto. */
const newRouteId = () => uuidV7(Date.now(), getRandomBytes(10))

/** The help of the Kind field for the chosen kind. */
function kindHelp(kind: SendRouteKind): string | null {
  if (kind === 'direct') {
    return 'Orbit delivers to each recipient’s mail server on port 25. This server must be allowed to send on port 25, and the PTR record of its address must name the mail hostname, or receivers mark the mail as spam. The Health button on Admin → Mail checks it.'
  }
  const made = generatedSecrets(kind).map((name) => (SECRET_LABELS[name] ?? name).toLowerCase())
  if (made.length === 0) return null
  return `Orbit makes the ${made.join(' and ')} and shows ${made.length > 1 ? 'them' : 'it'} once, after you add the route.`
}

/** A label with the pink `*` and an info tip, above a control that is not an `Input`. */
function ControlLabel({ label, required, help }: { label: string; required: boolean; help?: string | null }) {
  if (help) return <LabelTip label={label} text={help} required={required} />
  return (
    <FieldLabel>
      {label}
      {required ? <Text className="font-noto-semibold text-primary">{'  *'}</Text> : null}
    </FieldLabel>
  )
}

/** The inputs of `fields`; secret inputs stay empty, and a set secret says so. */
function RouteFields({ kind, fields, settings, secrets, secretsSet, onSetting, onSecret }: { kind: SendRouteKind; fields: FieldSpec[]; settings: FormValues; secrets: FormValues; secretsSet: string[]; onSetting: (name: string, value: string) => void; onSecret: (name: string, value: string) => void }) {
  return (
    <>
      {fields.map((field) => {
        const values = field.group === 'settings' ? settings : secrets
        const set = field.group === 'settings' ? onSetting : onSecret
        const isSet = field.group === 'secrets' && secretsSet.includes(field.name)
        const required = field.required && !isSet
        const key = `${field.group}-${field.name}`
        if (field.input === 'select' && field.options) {
          return (
            <View key={key} className="gap-2">
              <ControlLabel label={field.label} required={required} help={field.help} />
              <Select label={field.label} value={values[field.name] || field.defaultValue || ''} options={field.options} onChange={(value) => set(field.name, value)} />
              <ProviderLinks kind={kind} purpose="send" field={field.name} />
            </View>
          )
        }
        return (
          <View key={key} className="gap-2">
            <Input
              label={field.label}
              required={required}
              help={field.help}
              secureTextEntry={field.input === 'password'}
              keyboardType={field.input === 'number' ? 'number-pad' : 'default'}
              autoCapitalize="none"
              autoCorrect={false}
              autoComplete="off"
              placeholder={isSet ? 'Set. Leave empty to keep it.' : field.placeholder}
              value={values[field.name] ?? ''}
              onChangeText={(value) => set(field.name, value)}
            />
            <ProviderLinks kind={kind} purpose="send" field={field.name} />
          </View>
        )
      })}
    </>
  )
}

function MaxBytesField({ kind, value, onChange }: { kind: SendRouteKind; value: string; onChange: (value: string) => void }) {
  return (
    <Input
      label="Largest message (MB)"
      help={`Messages are refused above this size, as sent (attachments grow by a third). Empty: ${megabytes(defaultMaxBytes(kind))}, the most this kind takes.`}
      keyboardType="decimal-pad"
      autoComplete="off"
      placeholder={String(defaultMaxBytes(kind) / 1_000_000)}
      value={value}
      onChangeText={onChange}
      error={maxBytesValid(value) ? undefined : 'Enter a size in MB above 0, or leave it empty.'}
    />
  )
}

/** Add a send route (kind, name, its secrets and settings), or change one: secrets left empty stay as they are. */
export function SendRouteForm({
  scope,
  route,
  directAvailable,
  eventsBaseUrl,
  onClose,
  onCreated,
}: {
  scope: ProviderScope
  route?: MailSendRouteView
  directAvailable: boolean
  /** The list's `events_base_url`: with it, a new route of a kind that needs a webhook secret shows its events URL. */
  eventsBaseUrl?: string
  onClose: () => void
  /** The new route, with the secrets the server made (`generated`, shown once). */
  onCreated?: (created: MailSendRouteView) => void
}) {
  const { create, update } = useSendRouteWrites(scope)
  const firstKind: SendRouteKind = route && isSendRouteKind(route.kind) ? route.kind : directAvailable ? 'direct' : 'smtp'
  const [kind, setKind] = useState<SendRouteKind>(firstKind)
  const fields = routeFormFields(kind, scope)
  // the new route's id, made here so the events URL can be shown before the route exists
  const [newId] = useState(newRouteId)
  const eventsUrl = !route && eventsBaseUrl && eventsUrlInForm(fields) ? `${eventsBaseUrl}/${kind}/${newId}` : null
  const [name, setName] = useState(route?.name ?? ROUTE_LABELS[firstKind])
  const [settings, setSettings] = useState<FormValues>(() => initialSettingValues(fields, route?.settings))
  const [secrets, setSecrets] = useState<FormValues>({})
  const [maxBytes, setMaxBytes] = useState(() => {
    const bytes = route?.settings?.max_message_bytes ?? 0
    return bytes > 0 && bytes !== defaultMaxBytes(firstKind) ? String(bytes / 1_000_000) : ''
  })
  // the web's confirmation of **Remove login**, inside this dialog
  const [confirmingLogout, setConfirmingLogout] = useState(false)
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error
  const ready = routeFormReady(fields, { settings, secrets, maxBytes, editing: Boolean(route) })
  const hasLogin = kind === 'smtp' && (route?.secrets_set.some((secret) => secret === 'username' || secret === 'password') ?? false)
  const help = route ? null : kindHelp(kind)
  // Orbit Cloud never sends from its own servers, so it has no Direct route.
  const cloud = useAuthOptions().data?.billing_enabled ?? false
  const kindOptions = SEND_ROUTE_KINDS.filter((value) => value !== 'direct' || !cloud || route?.kind === 'direct').map((value) => ({
    value,
    label: value === 'direct' && !directAvailable ? `${ROUTE_LABELS[value]} (needs ORBIT_MAIL_HOSTNAME)` : ROUTE_LABELS[value],
  }))

  function chooseKind(next: SendRouteKind) {
    // the web disables this option
    if (next === 'direct' && !directAvailable) return
    setKind(next)
    // keep a name the user typed; replace the previous kind's prefilled one
    setName((current) => (current.trim() === '' || current === ROUTE_LABELS[kind] ? ROUTE_LABELS[next] : current))
    setSecrets({})
    setSettings(initialSettingValues(routeFormFields(next, scope), null))
    setMaxBytes('')
  }

  function submit() {
    if (saving || !ready) return
    const body = { name: name.trim() || ROUTE_LABELS[kind], settings: settingsBody(fields, settings, maxBytes, route?.settings) }
    const given = secretsBody(fields, secrets)
    if (route) {
      update.mutate({ routeId: route.id, body: { ...body, secrets: Object.keys(given).length > 0 ? given : null } }, { onSuccess: onClose })
      return
    }
    // the server makes the webhook secrets it checks (Postmark, Cloudflare) and answers with them once
    create.mutate({ id: newId, kind, ...body, secrets: given }, {
      onSuccess: (created) => {
        onCreated?.(created)
        onClose()
      },
    })
  }

  function removeLogin() {
    if (!route) return
    setConfirmingLogout(false)
    // a failure shows under the form, as for Save
    update.mutate({ routeId: route.id, body: { clear_login: true } }, { onSuccess: onClose })
  }

  return (
    <Dialog
      visible
      title={route ? `Change ${route.name}` : 'Add an outgoing provider'}
      description="Outgoing mail: how Orbit sends the mail of the domains that use it."
      onClose={() => (saving ? undefined : onClose())}
      footer={
        <>
          <Button label="Cancel" variant="outline" disabled={saving} onPress={onClose} />
          <Button label={saving ? 'Saving…' : route ? 'Save' : 'Add provider'} disabled={saving || !ready} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <View className="gap-2">
          <ControlLabel label="Kind" required={false} help={help} />
          <Select label="Kind" value={kind} options={kindOptions} disabled={Boolean(route)} onChange={chooseKind} />
        </View>
        <Input label="Name" autoComplete="off" autoCorrect={false} maxLength={80} placeholder={ROUTE_LABELS[kind]} value={name} onChangeText={setName} />
        <ProviderLinks kind={kind} purpose="send" />
        {eventsUrl ? <CopyRow label="Events URL" value={eventsUrl} /> : null}
        <RouteFields
          kind={kind}
          fields={fields}
          settings={settings}
          secrets={secrets}
          secretsSet={route?.secrets_set ?? []}
          onSetting={(field, value) => setSettings((current) => ({ ...current, [field]: value }))}
          onSecret={(field, value) => setSecrets((current) => ({ ...current, [field]: value }))}
        />
        {hasLogin && route ? (
          confirmingLogout ? (
            <Callout title={`Remove the login of ${route.name}?`} description="Orbit connects to the relay without a user name and password.">
              <View className="flex-row flex-wrap justify-end gap-2">
                <Button label="Cancel" size="sm" variant="outline" onPress={() => setConfirmingLogout(false)} />
                <Button label="Remove login" size="sm" variant="destructive" disabled={saving} onPress={removeLogin} />
              </View>
            </Callout>
          ) : (
            <View className="flex-row">
              <Button label="Remove login" size="sm" variant="destructive" disabled={saving} onPress={() => setConfirmingLogout(true)} />
            </View>
          )
        ) : null}
        <MaxBytesField kind={kind} value={maxBytes} onChange={setMaxBytes} />
        {error ? <Note error>{mailProblem(error, 'The route was not saved.')}</Note> : null}
      </View>
    </Dialog>
  )
}

/** **Also send through this provider**: a route with the provider's account; the form asks only for what it lacks. */
export function ProviderSendRouteForm({ scope, provider, onClose, onCreated }: { scope: ProviderScope; provider: MailProviderView; onClose: () => void; onCreated?: (created: MailSendRouteView) => void }) {
  const { createFromProvider: create } = useSendRouteWrites(scope)
  const kind = routeKindOfProvider(provider.kind)
  const fields = routeFormFields(kind, scope, true)
  const [settings, setSettings] = useState<FormValues>(() => initialSettingValues(fields, null))
  const [secrets, setSecrets] = useState<FormValues>({})
  const [maxBytes, setMaxBytes] = useState('')
  const ready = routeFormReady(fields, { settings, secrets, maxBytes, editing: false })

  function submit() {
    if (create.isPending || !ready) return
    create.mutate(
      { providerId: provider.id, body: { secrets: secretsBody(fields, secrets), settings: settingsBody(fields, settings, maxBytes, null) } },
      {
        onSuccess: (route) => {
          onCreated?.(route)
          onClose()
        },
      },
    )
  }

  return (
    <Dialog
      visible
      title={`Also send through ${provider.name}`}
      description={`A send route with your ${ROUTE_LABELS[kind]} account. Orbit reuses what the provider already has.`}
      onClose={() => (create.isPending ? undefined : onClose())}
      footer={
        <>
          <Button label="Cancel" variant="outline" disabled={create.isPending} onPress={onClose} />
          <Button label={create.isPending ? 'Adding…' : 'Add route'} disabled={create.isPending || !ready} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <RouteFields
          kind={kind}
          fields={fields}
          settings={settings}
          secrets={secrets}
          secretsSet={[]}
          onSetting={(field, value) => setSettings((current) => ({ ...current, [field]: value }))}
          onSecret={(field, value) => setSecrets((current) => ({ ...current, [field]: value }))}
        />
        <MaxBytesField kind={kind} value={maxBytes} onChange={setMaxBytes} />
        {create.error ? <Note error>{mailProblem(create.error, 'The route was not added.')}</Note> : null}
      </View>
    </Dialog>
  )
}

/** The web's `Alert`: a bordered box with a title and a line; `destructive` for a problem. */
export function Callout({ title, description, destructive = false, children }: { title: string; description?: ReactNode; destructive?: boolean; children?: ReactNode }) {
  return (
    <View className={`gap-1 rounded-lg border p-3 ${destructive ? 'border-destructive/40 bg-destructive/5' : 'border-border bg-card'}`}>
      <Text className={`font-noto-medium text-[13px] leading-[19px] ${destructive ? 'text-destructive' : 'text-foreground'}`}>{title}</Text>
      {description ? <Text className={`text-[13px] leading-[19px] ${destructive ? 'text-destructive/90' : 'text-muted-foreground'}`}>{description}</Text> : null}
      {children}
    </View>
  )
}

/** The web's `CopyButton`: copies `text`, then says Copied (or that it failed). */
export function CopyButton({ text, label }: { text: string; label: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle')
  return (
    <SmallOutlineButton
      label={state === 'copied' ? 'Copied' : state === 'failed' ? 'Failed' : 'Copy'}
      accessibilityLabel={`Copy ${label}`}
      icon="copy"
      muted
      state={state}
      onPress={() => void Clipboard.setStringAsync(text).then((done) => setState(done ? 'copied' : 'failed'), () => setState('failed'))}
    />
  )
}

/** One value to paste at the provider, with a copy button. */
export function CopyRow({ label, value }: { label: string; value: string }) {
  return (
    <View className="gap-1">
      <Text className="font-noto-medium text-[12px] text-muted-foreground">{label}</Text>
      <View className="flex-row items-start gap-1 rounded-md border border-border bg-muted/40 px-2 py-1">
        <Text selectable className="min-w-0 flex-1 py-0.5 font-mono text-[12px] leading-[17px]">
          {value}
        </Text>
        <CopyButton text={value} label={label} />
      </View>
    </View>
  )
}

/**
 * What to set up at the provider for a route: the events URL (from the server), the steps and (Cloudflare) the queue
 * Worker. The secrets the server made (`route.generated`) are only in the answer that created the route.
 */
export function RouteSetup({ route }: { route: MailSendRouteView }) {
  if (!isSendRouteKind(route.kind)) return null
  const kind = route.kind
  const generated = route.generated ?? {}
  const secrets = Object.entries(generated)
  const login = generated.webhook_user && generated.webhook_password ? { user: generated.webhook_user, password: generated.webhook_password } : null
  const fromProvider = route.event_secret_from_provider
  return (
    <View className="gap-4">
      {secrets.length > 0 ? <Callout title="Copy these now. Orbit does not show them again." description="If you lose them, remove the route and add it again." /> : null}
      {route.events_url ? <CopyRow label="Events URL" value={eventsUrlWithLogin(route.events_url, login)} /> : null}
      {route.events_url && missingEventSecret(route) ? (
        <Callout title="Orbit refuses this route’s events until it has the webhook’s signing secret." description="Add the webhook with this URL, then Edit the route and paste its signing secret." />
      ) : null}
      {kind === 'postmark' && !login ? (
        <Text className="text-[12px] leading-[17px] text-muted-foreground">
          {route.provider_id
            ? 'Put the user name and password of the provider’s inbound webhook in this URL (https://user:password@…).'
            : 'Put the webhook user name and password Orbit showed when you added the route in this URL (https://user:password@…).'}
        </Text>
      ) : null}
      {secrets.map(([name, value]) => <CopyRow key={name} label={SECRET_LABELS[name] ?? name} value={value} />)}
      <View className="gap-1.5">
        {routeSetupSteps(kind, fromProvider).map((step, index) => (
          <View key={step} className="flex-row gap-2">
            <Text className="w-4 text-right text-[13px] leading-[19px] text-muted-foreground">{index + 1}.</Text>
            <Text className="min-w-0 flex-1 text-[13px] leading-[19px] text-foreground">{step}</Text>
          </View>
        ))}
      </View>
      {kind === 'cloudflare' && !fromProvider ? (
        <View className="gap-1">
          <View className="flex-row items-center justify-between gap-2">
            <Text className="font-noto-medium text-[12px] text-muted-foreground">Queue Worker script</Text>
            <CopyButton text={CLOUDFLARE_EVENTS_WORKER} label="Script" />
          </View>
          <ScrollView nestedScrollEnabled className="max-h-72 rounded-md border border-border bg-muted/40" contentContainerClassName="p-2">
            <Text selectable className="font-mono text-[12px] leading-[17px]">
              {CLOUDFLARE_EVENTS_WORKER}
            </Text>
          </ScrollView>
        </View>
      ) : null}
    </View>
  )
}
