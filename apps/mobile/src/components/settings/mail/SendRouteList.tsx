/**
 * The web's `MailSendingPage.tsx` parts that other pages share: a workspace's tier and limits (`PolicyRows`), and the
 * outgoing routes of a scope (`OutgoingRoutes`, a section of Settings → Mail → Providers and of Admin → Mail).
 */
import type { MailSendPolicy } from '@orbit/api/generated/types.gen'
import { useState } from 'react'
import { View } from 'react-native'

import type { MailSendRouteTest, MailSendRouteView, ProviderScope } from '../../../data/mail-sending-api'
import { count, dateTime, defaultMaxBytes, isSendRouteKind, limitRows, mailProblem, megabytes, percent, routeLabel, since, spfHint, TIER_LABELS } from '../../../data/mail-settings'
import { useSendRoutes, useSendRouteWrites, useSetSendDefault } from '../../../hooks/mail-sending'
import { presenceColors, useTheme } from '../../../theme'
import { Button, Icon, type IconName, Input, LabelTip, Text } from '../../../ui'
import { ConfirmDialog, type ConfirmRequest, Dialog } from '../Dialog'
import { Badge, Note, SettingsRow } from '../Shell'
import { RouteSetup, SendRouteForm } from './SendRouteForm'

/** A workspace's tier, limits, use and rates (read only), as the rows of a flush card. */
export function PolicyRows({ policy }: { policy: MailSendPolicy }) {
  return (
    <View>
      <SettingsRow first>
        <Text className="min-w-0 flex-1 font-noto-medium text-[13px] text-foreground">Tier</Text>
        <Badge label={TIER_LABELS[policy.tier]} tone="outline" />
      </SettingsRow>
      {limitRows(policy.limits).map((row) => (
        <SettingsRow key={row.label}>
          <Text className="min-w-0 flex-1 font-noto-medium text-[13px] text-foreground">{row.label}</Text>
          <Text className="shrink text-right text-[13px] text-muted-foreground">{row.value}</Text>
        </SettingsRow>
      ))}
      <SettingsRow>
        <Text className="min-w-0 flex-1 font-noto-medium text-[13px] text-foreground">Sent</Text>
        <Text className="shrink text-right text-[13px] text-muted-foreground">
          {policy.use_hour.toLocaleString('en-US')} in the last hour, {policy.use_day.toLocaleString('en-US')} today
        </Text>
      </SettingsRow>
      <SettingsRow>
        <View className="min-w-0 flex-1">
          <LabelTip label="Last 7 days" text="Sending through a shared route pauses when, over at least 100 of its recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain." className="font-noto-medium text-[13px] text-foreground" />
        </View>
        <Text className="shrink text-right text-[13px] text-muted-foreground">
          {count(policy.recipients_7d, 'recipient', 'recipients')} · {percent(policy.hard_bounce_rate_7d)} bounced · {percent(policy.complaint_rate_7d)} complained
        </Text>
      </SettingsRow>
    </View>
  )
}

/**
 * The outgoing routes of `scope`, a section of the Providers card. A workspace lists the server's routes too, read
 * only. Each own route has its setup, **Send test**, **Change** and **Remove**. `hostname` (the root user knows it)
 * fills the SPF of a direct route. `adding` opens the form of a new route (the card's **Add** button).
 */
export function OutgoingRoutes({ scope, hostname, adding, onAddClose }: { scope: ProviderScope; hostname?: string | null; adding: boolean; onAddClose: () => void }) {
  const routes = useSendRoutes(scope)
  const [editing, setEditing] = useState<MailSendRouteView | null>(null)
  const [created, setCreated] = useState<MailSendRouteView | null>(null)
  const directAvailable = routes.data?.direct_available ?? false

  return (
    <>
      {routes.isPending ? (
        <SettingsRow first>
          <Note loading>Loading routes…</Note>
        </SettingsRow>
      ) : null}
      {routes.isError ? (
        <SettingsRow first>
          <View className="min-w-0 flex-1">
            <Note error>{mailProblem(routes.error, 'The routes could not be loaded.')}</Note>
          </View>
          <Button label="Retry" size="sm" variant="ghost" onPress={() => void routes.refetch()} />
        </SettingsRow>
      ) : null}
      {routes.data?.routes.length === 0 ? (
        <SettingsRow first>
          <View className="min-w-0 flex-1">
            <Note>{scope === 'instance' ? 'No outgoing providers. Without one, no workspace can send mail.' : 'No outgoing providers. Your server admin has not set one up either, so your domains cannot send yet.'}</Note>
          </View>
        </SettingsRow>
      ) : null}
      {routes.data?.routes.map((route, index) => (
        <SendRouteRow key={route.id} first={index === 0} scope={scope} route={route} hostname={hostname} onEdit={() => setEditing(route)} />
      ))}
      {adding ? <SendRouteForm scope={scope} directAvailable={directAvailable} eventsBaseUrl={routes.data?.events_base_url} onClose={onAddClose} onCreated={setCreated} /> : null}
      {editing ? <SendRouteForm scope={scope} route={editing} directAvailable={directAvailable} onClose={() => setEditing(null)} /> : null}
      {created ? <RouteCreatedDialog created={created} onClose={() => setCreated(null)} /> : null}
    </>
  )
}

/** After a route is added: what to set up at the provider, with the secrets the server made (shown once). */
export function RouteCreatedDialog({ created, onClose }: { created: MailSendRouteView; onClose: () => void }) {
  return (
    <Dialog visible title={`Set up ${created.name}`} description={`Do these steps for ${routeLabel(created.kind)}.`} onClose={onClose} footer={<Button label="Done" onPress={onClose} />}>
      <RouteSetup route={created} />
    </Dialog>
  )
}

function SendRouteRow({ first, scope, route, hostname, onEdit }: { first: boolean; scope: ProviderScope; route: MailSendRouteView; hostname?: string | null; onEdit: () => void }) {
  const { remove } = useSendRouteWrites(scope)
  const setDefault = useSetSendDefault()
  const [setupOpen, setSetupOpen] = useState(false)
  const [testOpen, setTestOpen] = useState(false)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  // the web's toasts: the last write that failed
  const [problem, setProblem] = useState<string | null>(null)
  // a server route listed to a workspace: its name and kind only
  const own = route.settings !== null && route.settings !== undefined
  const kind = isSendRouteKind(route.kind) ? route.kind : null
  const spf = kind ? spfHint(kind, route.settings, hostname) : null
  const maxBytes = route.settings?.max_message_bytes || (kind ? defaultMaxBytes(kind) : 0)
  // only the server's own routes can be the default, and only Admin → Mail sets it
  const canSetDefault = own && scope === 'instance' && route.scope === 'instance'

  const removeRoute = () =>
    setConfirm({
      title: `Remove ${route.name}?`,
      description: 'Orbit stops sending through it. A route that a domain uses, or the default route, cannot be removed.',
      confirmLabel: 'Remove',
      danger: true,
      onConfirm: () => {
        setProblem(null)
        remove.mutate(route.id, { onError: (error) => setProblem(mailProblem(error, 'The route could not be removed.')) })
      },
    })

  function saveDefault(routeId: string | null) {
    setProblem(null)
    setDefault.mutate(routeId, { onError: (error) => setProblem(mailProblem(error, 'The default route could not be saved.')) })
  }

  return (
    <View className={`gap-3 border-border px-4 py-3 ${first ? '' : 'border-t'}`}>
      <View className="gap-0.5">
        <View className="flex-row flex-wrap items-center gap-2">
          <Text className="font-noto-medium text-[13px] text-foreground">{route.name}</Text>
          <Badge label={routeLabel(route.kind)} tone="outline" />
          {scope === 'workspace' && route.scope === 'instance' ? <Badge label="Server" tone="secondary" /> : null}
          {route.is_default ? <Badge label="Default" tone="secondary" /> : null}
        </View>
        {own ? (
          <Text className="text-[12px] leading-[17px] text-muted-foreground">
            {count(route.domain_count, 'domain', 'domains')}
            {maxBytes > 0 ? ` · Up to ${megabytes(maxBytes)}` : null}
            {route.last_sent_at ? ` · Last sent ${since(route.last_sent_at)}` : ' · Nothing sent yet'}
          </Text>
        ) : null}
        {own && spf ? (
          <Text className="text-[12px] leading-[17px] text-muted-foreground">
            SPF: <Text selectable className="font-mono text-[12px] text-muted-foreground">{spf}</Text>
          </Text>
        ) : null}
        {own && kind && !spf ? <Text className="text-[12px] leading-[17px] text-muted-foreground">SPF: set the relay’s SPF entry on the route.</Text> : null}
        {route.last_error ? <Text className="text-[12px] leading-[17px] text-destructive">Last error: {route.last_error}</Text> : null}
      </View>
      {own ? (
        // the web's row buttons sit beside the text; a phone row has no room, so they wrap under it
        <View className="flex-row flex-wrap items-center gap-1">
          <Button label="Send test" size="sm" variant="outline" onPress={() => setTestOpen(true)} />
          <Button label={setupOpen ? 'Hide setup' : 'Setup'} size="sm" variant="ghost" onPress={() => setSetupOpen((value) => !value)} />
          {canSetDefault ? (
            <Button label={route.is_default ? 'Unset default' : 'Set as default'} size="sm" variant="ghost" disabled={setDefault.isPending} onPress={() => saveDefault(route.is_default ? null : route.id)} />
          ) : null}
          <Button label="Change" size="sm" variant="ghost" onPress={onEdit} />
          <Button label="Remove" size="sm" variant="destructive" disabled={remove.isPending} onPress={removeRoute} />
        </View>
      ) : null}
      {problem ? <Note error>{problem}</Note> : null}
      {setupOpen ? <RouteSetup route={route} /> : null}
      {testOpen ? <SendTestDialog scope={scope} route={route} onClose={() => setTestOpen(false)} /> : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </View>
  )
}

const STEP_ICONS: Record<'ok' | 'failed' | 'skipped', IconName> = { ok: 'checkCircle', failed: 'closeCircle', skipped: 'minusCircle' }
const STEP_WORDS = { ok: 'OK', failed: 'Failed', skipped: 'Skipped' } as const

/** **Send test**: a message to an address the admin types, and the result of each step. */
function SendTestDialog({ scope, route, onClose }: { scope: ProviderScope; route: MailSendRouteView; onClose: () => void }) {
  const { colors } = useTheme()
  const { test } = useSendRouteWrites(scope)
  const [to, setTo] = useState('')
  const [result, setResult] = useState<MailSendRouteTest | null>(null)

  function submit() {
    const address = to.trim()
    if (!address || test.isPending) return
    setResult(null)
    test.mutate({ routeId: route.id, to: address }, { onSuccess: setResult })
  }

  const stepColor = { ok: presenceColors.online, failed: colors.destructive, skipped: colors.mutedForeground }
  return (
    <Dialog visible title={`Send a test through ${route.name}`} description="Orbit sends a short message and shows the result of each step." onClose={onClose} footer={<Button label="Close" variant="outline" onPress={onClose} />}>
      <View className="gap-3">
        <Input
          accessibilityLabel="Send a test message to"
          keyboardType="email-address"
          autoCapitalize="none"
          autoCorrect={false}
          autoComplete="email"
          autoFocus
          placeholder="you@example.com"
          value={to}
          onChangeText={setTo}
          onSubmitEditing={submit}
          returnKeyType="send"
          accessory={<Button label={test.isPending ? 'Sending…' : 'Send test'} variant="outline" disabled={!to.trim() || test.isPending} onPress={submit} />}
        />
        {test.isError ? <Note error>{mailProblem(test.error, 'The test could not run.')}</Note> : null}
        {result ? (
          <View accessibilityLabel={`Test of ${route.name}`} className="gap-1.5">
            {result.steps.map((step) => (
              <View key={step.name} className="flex-row items-start gap-2">
                <View className="mt-0.5">
                  <Icon name={STEP_ICONS[step.status]} size={16} color={stepColor[step.status]} />
                </View>
                <View accessibilityLabel={`${step.name}: ${STEP_WORDS[step.status]}`} className="min-w-0 flex-1">
                  <Text className="font-noto-medium text-[13px] text-foreground">{step.name}</Text>
                  {step.detail ? <Text className="text-[12px] leading-[17px] text-muted-foreground">{step.detail}</Text> : null}
                </View>
              </View>
            ))}
          </View>
        ) : null}
        {route.last_sent_at ? <Text className="text-[12px] text-muted-foreground">Last message sent {dateTime(route.last_sent_at)}.</Text> : null}
      </View>
    </Dialog>
  )
}
