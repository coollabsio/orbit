import * as Clipboard from 'expo-clipboard'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest, Dialog } from '../../../components/settings/Dialog'
import { CheckRow, ProjectPicker } from '../../../components/settings/ProjectPicker'
import { Badge, FieldLabel, Note, RowIcon, SettingsCard, SettingsRow, SettingsShell, Toggle, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { relativeTime } from '../../../data/format'
import { deliveryAnswer, deliveryState, EVENT_TYPES, mayRetry, webhookFailure, webhookSummary, type WebhookDelivery, type WebhookEventType, type WebhookSubscription } from '../../../data/webhooks-api'
import { useRetryDelivery, useWebhookDeliveries, useWebhooks, useWebhookWrites } from '../../../hooks/webhooks'
import { useOrbit } from '../../../orbit-context'
import { useProjects } from '../../../queries'
import { Button, Input, Text } from '../../../ui'

/** The web's Settings → Webhooks: signed task events to a URL, with each webhook's delivery log. */
export default function WebhooksScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('integrations.manage')
  const webhooks = useWebhooks(canManage)
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={canManage ? () => webhooks.refetch() : undefined}>
      {canManage ? (
        <Webhooks />
      ) : (
        <SettingsCard title="Webhooks">
          <Text className="text-[14px] leading-[20px] text-muted-foreground">Only workspace owners and administrators can manage webhooks.</Text>
        </SettingsCard>
      )}
    </SettingsShell>
  )
}

function Webhooks() {
  const webhooks = useWebhooks(true)
  const projectsQuery = useProjects()
  const projects = projectsQuery.data ?? []
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const { update, remove, rotate } = useWebhookWrites()
  const [editing, setEditing] = useState<WebhookSubscription | 'new'>()
  const [secret, setSecret] = useState<{ name: string; value: string }>()
  const [copied, setCopied] = useState<'copied' | 'failed'>()
  const [openDeliveries, setOpenDeliveries] = useState<string>()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  // the web's toasts: the last write that failed
  const [problem, setProblem] = useState<string>()

  const showSecret = (name: string, value: string) => {
    setSecret({ name, value })
    setCopied(undefined)
  }
  const failed = (fallback: string) => (error: unknown) => setProblem(webhookFailure(error, fallback))
  const status = webhooks.isPending ? 'Loading webhooks…' : webhooks.isError ? 'Webhooks could not be loaded.' : webhooks.data.length === 0 ? 'No webhooks yet.' : null

  return (
    <>
      <SettingsCard
        title="Webhooks"
        description="Orbit posts signed JSON to your URL when a task in a selected project is created, changed or moved to the trash. Deliveries are retried for about a day."
        actions={<Button label="Add webhook" onPress={() => setEditing('new')} />}
      >
        <View className="gap-3">
          {secret ? (
            <View accessibilityLiveRegion="polite" className="gap-2.5 rounded-lg border border-primary bg-primary/10 p-3">
              <Text className="font-noto-semibold text-[12px] leading-[16px]">Copy the signing secret of {secret.name} now. You cannot see it again.</Text>
              <View className="flex-row items-center gap-2">
                <Text selectable className="min-w-0 flex-1 font-mono text-[12px] leading-[16px]">
                  {secret.value}
                </Text>
                <Button
                  label={copied === 'copied' ? 'Copied' : 'Copy'}
                  icon="copy"
                  variant="outline"
                  onPress={() => void Clipboard.setStringAsync(secret.value).then((done) => setCopied(done ? 'copied' : 'failed'), () => setCopied('failed'))}
                />
              </View>
              {copied === 'failed' ? <Note error>Could not copy the secret.</Note> : null}
            </View>
          ) : (
            <Text className="text-[14px] leading-[20px] text-muted-foreground">
              Check each request with its <Text className="font-mono text-[13px] text-muted-foreground">webhook-signature</Text> header, as the docs on webhooks describe.
            </Text>
          )}
          {problem ? <Note error>{problem}</Note> : null}
        </View>
      </SettingsCard>
      <SettingsCard title="Subscriptions" flush>
        {status ? (
          <SettingsRow first>
            {webhooks.isPending ? <Note loading>{status}</Note> : <Text className={`text-[14px] leading-[20px] ${webhooks.isError ? 'text-destructive' : 'text-foreground'}`}>{status}</Text>}
          </SettingsRow>
        ) : null}
        {webhooks.data?.map((webhook, index) => (
          <View key={webhook.id} className={index === 0 ? '' : 'border-t border-border'}>
            <SettingsRow first>
              <RowIcon icon="global" />
              <View className="min-w-0 flex-1 gap-0.5">
                <View className="flex-row flex-wrap items-center gap-2">
                  <Text className="font-noto-medium text-[13px] text-foreground">{webhook.name}</Text>
                  {webhook.enabled ? null : <Badge label="Off" tone="secondary" />}
                </View>
                <Text numberOfLines={1} className="text-[12px] text-muted-foreground/70">
                  {webhook.url}
                </Text>
                <Text className="text-[12px] text-muted-foreground/70">{webhookSummary(webhook, projectNames)}</Text>
              </View>
              <Toggle
                label={`Send events to ${webhook.name}`}
                value={webhook.enabled}
                disabled={update.isPending}
                onChange={(enabled) => {
                  setProblem(undefined)
                  update.mutate({ webhookId: webhook.id, body: { enabled } }, { onError: failed('The webhook could not be changed.') })
                }}
              />
            </SettingsRow>
            {/* the web's row buttons sit beside the text; a phone row has no room, so they wrap under it */}
            <View className="flex-row flex-wrap gap-1 px-3 pb-2">
              <Button label={openDeliveries === webhook.id ? 'Hide deliveries' : 'Deliveries'} variant="ghost" onPress={() => setOpenDeliveries(openDeliveries === webhook.id ? undefined : webhook.id)} />
              <Button label="Edit" variant="ghost" onPress={() => setEditing(webhook)} />
              <Button
                label="Rotate secret"
                variant="ghost"
                disabled={rotate.isPending}
                onPress={() =>
                  setConfirm({
                    title: `Rotate the secret of ${webhook.name}?`,
                    description: 'Deliveries are signed with the new secret at once, retries included. Update your endpoint before the next event.',
                    confirmLabel: 'Rotate',
                    onConfirm: () => {
                      setProblem(undefined)
                      rotate.mutate(webhook.id, { onSuccess: (value) => showSecret(webhook.name, value), onError: failed('The secret could not be rotated.') })
                    },
                  })
                }
              />
              <Button
                label="Delete"
                variant="ghost"
                disabled={remove.isPending}
                onPress={() =>
                  setConfirm({
                    title: `Delete ${webhook.name}?`,
                    description: 'Pending deliveries are not sent, and the delivery log is removed.',
                    confirmLabel: 'Delete',
                    danger: true,
                    onConfirm: () => {
                      setProblem(undefined)
                      remove.mutate(webhook.id, { onError: () => setProblem('The webhook could not be deleted.') })
                    },
                  })
                }
              />
            </View>
            {openDeliveries === webhook.id ? <Deliveries webhookId={webhook.id} /> : null}
          </View>
        ))}
      </SettingsCard>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      {editing ? (
        <WebhookDialog
          webhook={editing === 'new' ? undefined : editing}
          projects={projects}
          projectsLoading={projectsQuery.isPending}
          onClose={() => setEditing(undefined)}
          onCreated={showSecret}
        />
      ) : null}
    </>
  )
}

/** The web's delivery table as a list: a phone has no room for its six columns. */
function Deliveries({ webhookId }: { webhookId: string }) {
  const deliveries = useWebhookDeliveries(webhookId)
  const retry = useRetryDelivery(webhookId)
  return (
    <View className="gap-2 border-t border-border bg-muted/30 px-4 py-3">
      {deliveries.isPending ? <Note loading>Loading deliveries…</Note> : null}
      {deliveries.isError ? <Note error>Deliveries could not be loaded.</Note> : null}
      {deliveries.data?.length === 0 ? <Note>No deliveries in the last 30 days.</Note> : null}
      {deliveries.data?.map((delivery, index) => <DeliveryRow key={delivery.id} delivery={delivery} first={index === 0} retrying={retry.isPending} onRetry={() => retry.mutate(delivery.id)} />)}
      {retry.isError ? <Note error>The delivery could not be sent again.</Note> : null}
    </View>
  )
}

function DeliveryRow({ delivery, first, retrying, onRetry }: { delivery: WebhookDelivery; first: boolean; retrying: boolean; onRetry: () => void }) {
  const state = deliveryState(delivery)
  const attempts = `${delivery.attempt_count} ${delivery.attempt_count === 1 ? 'attempt' : 'attempts'}`
  return (
    <View className={`flex-row items-center gap-3 border-border/60 ${first ? '' : 'border-t pt-2'}`}>
      <View className="min-w-0 flex-1 gap-1">
        <View className="flex-row flex-wrap items-center gap-2">
          <Text className="font-mono text-[12px] leading-[16px] text-foreground">{delivery.event_type}</Text>
          <Badge label={state.label} tone={state.tone} />
        </View>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{`${attempts} · ${deliveryAnswer(delivery)} · ${relativeTime(delivery.created_at)}`}</Text>
      </View>
      {mayRetry(delivery) ? <Button label="Retry" size="sm" variant="outline" disabled={retrying} onPress={onRetry} /> : null}
    </View>
  )
}

function WebhookDialog({ webhook, projects, projectsLoading, onClose, onCreated }: { webhook?: WebhookSubscription; projects: { id: string; name: string }[]; projectsLoading: boolean; onClose: () => void; onCreated: (name: string, secret: string) => void }) {
  const { create, update } = useWebhookWrites()
  const [name, setName] = useState(webhook?.name ?? '')
  const [url, setUrl] = useState(webhook?.url ?? '')
  const [eventTypes, setEventTypes] = useState<WebhookEventType[]>(webhook?.event_types ?? EVENT_TYPES.map((event) => event.value))
  const [projectIds, setProjectIds] = useState<string[]>(webhook?.project_ids ?? [])
  const [enabled, setEnabled] = useState(webhook?.enabled ?? true)
  const [error, setError] = useState<string>()
  const pending = create.isPending || update.isPending
  const ready = name.trim() !== '' && url.trim() !== '' && eventTypes.length > 0 && projectIds.length > 0

  const submit = () => {
    if (!ready || pending) return
    const body = { name: name.trim(), url: url.trim(), event_types: eventTypes, project_ids: projectIds, enabled }
    setError(undefined)
    const onError = (caught: unknown) => setError(webhookFailure(caught, webhook ? 'The webhook could not be saved.' : 'The webhook could not be created.'))
    if (webhook) {
      update.mutate({ webhookId: webhook.id, body }, { onSuccess: onClose, onError })
    } else {
      create.mutate(body, {
        onSuccess: (created) => {
          onCreated(created.name, created.signing_secret)
          onClose()
        },
        onError,
      })
    }
  }

  return (
    <Dialog
      visible
      title={webhook ? `Edit ${webhook.name}` : 'Add webhook'}
      description={webhook ? 'Changes apply to the next event.' : 'The signing secret is shown once, after you add the webhook.'}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={webhook ? 'Save' : 'Add webhook'} disabled={pending || !ready} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <Input label="Name" required value={name} maxLength={100} onChangeText={setName} placeholder="Billing sync" autoCorrect={false} />
        <Input label="URL" required value={url} maxLength={2048} onChangeText={setUrl} placeholder="https://example.com/orbit-webhook" keyboardType="url" autoCapitalize="none" autoCorrect={false} />
        <View accessibilityLabel="Events" className="gap-3">
          <FieldLabel>
            Events
            <Text className="font-noto-semibold text-primary">{'  *'}</Text>
          </FieldLabel>
          {EVENT_TYPES.map((option) => {
            const checked = eventTypes.includes(option.value)
            return <CheckRow key={option.value} label={option.label} checked={checked} onPress={() => setEventTypes(checked ? eventTypes.filter((other) => other !== option.value) : [...eventTypes, option.value])} />
          })}
        </View>
        <View className="gap-3">
          <FieldLabel>
            Projects
            <Text className="font-noto-semibold text-primary">{'  *'}</Text>
          </FieldLabel>
          <ProjectPicker projects={projects} selected={projectIds} onChange={setProjectIds} empty={projectsLoading ? 'Loading projects…' : projects.length ? undefined : 'This workspace has no active projects.'} />
        </View>
        <View className="flex-row items-center justify-between gap-3">
          <FieldLabel>Send events</FieldLabel>
          <Toggle label="Send events" value={enabled} onChange={setEnabled} />
        </View>
        {error ? <Note error>{error}</Note> : null}
      </View>
    </Dialog>
  )
}
