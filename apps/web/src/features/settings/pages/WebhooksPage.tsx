import { toast } from 'sonner'
import { useState, type FormEvent } from 'react'
import { Copy, Global } from 'reicon-react'
import { ApiProblem } from '@orbit/api/problem'
import type { WebhookDelivery, WebhookEventType, WebhookSubscription } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { ProjectPicker } from '@/components/common/ScopeAccess'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { LoadingText } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { relativeTime } from '@/lib/format'
import { useProjects } from '@/features/tasks/api/projects'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { RequiredMark, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCreateWebhook, useDeleteWebhook, useRetryWebhookDelivery, useRotateWebhookSecret, useUpdateWebhook, useWebhookDeliveries, useWebhooks } from '@/features/settings/api/webhooks'

const EVENT_TYPES: { value: WebhookEventType; label: string }[] = [
  { value: 'task.created', label: 'Task created' },
  { value: 'task.updated', label: 'Task updated' },
  { value: 'task.deleted', label: 'Task moved to the trash' },
]

const FIELD_MESSAGES: Record<string, string> = {
  name: 'Enter a name of 1 to 100 characters.',
  url: 'Enter a public https URL.',
  event_types: 'Select at least one event.',
  project_ids: 'Select at least one project.',
}

function failure(error: unknown, fallback: string) {
  if (!(error instanceof ApiProblem)) return fallback
  if (error.code === 'validation_failed') return FIELD_MESSAGES[error.detail] ?? fallback
  if (error.code === 'app_key_missing') return error.detail
  return fallback
}

const STATE_BADGES: Record<string, { label: string; variant: 'secondary' | 'outline' | 'destructive' | 'default' }> = {
  pending: { label: 'Pending', variant: 'secondary' },
  succeeded: { label: 'Delivered', variant: 'outline' },
  failed: { label: 'Failed', variant: 'destructive' },
  cancelled: { label: 'Cancelled', variant: 'secondary' },
}

export function WebhooksPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('integrations.manage')
  const webhooks = useWebhooks(workspace.id, canManage)
  const projectsQuery = useProjects(workspace.id, canManage)
  const projects = projectsQuery.data ?? []
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const updateWebhook = useUpdateWebhook(workspace.id)
  const deleteWebhook = useDeleteWebhook(workspace.id)
  const rotateSecret = useRotateWebhookSecret(workspace.id)
  const [editing, setEditing] = useState<WebhookSubscription | 'new'>()
  const [secret, setSecret] = useState<{ name: string; value: string }>()
  const [copied, setCopied] = useState(false)
  const [openDeliveries, setOpenDeliveries] = useState<string>()

  if (!canManage) return <SettingsCard title="Webhooks"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage webhooks.</p></SettingsCard>

  function showSecret(name: string, value: string) {
    setSecret({ name, value })
    setCopied(false)
  }

  async function rotate(webhook: WebhookSubscription) {
    if (!await confirmAction({ title: `Rotate the secret of ${webhook.name}?`, description: 'Deliveries are signed with the new secret at once, retries included. Update your endpoint before the next event.', confirmLabel: 'Rotate' })) return
    try {
      showSecret(webhook.name, await rotateSecret.mutateAsync(webhook.id))
    } catch (error) {
      toast.error(failure(error, 'The secret could not be rotated.'))
    }
  }

  return <>
    <SettingsCard title="Webhooks" description="Orbit posts signed JSON to your URL when a task in a selected project is created, changed or moved to the trash. Deliveries are retried for about a day." actions={<Button type="button" onClick={() => setEditing('new')}>Add webhook</Button>}>
      {secret ? <div className="grid gap-2.5 rounded-lg border border-primary bg-primary/10 p-3 text-xs" role="status">
        <strong>Copy the signing secret of {secret.name} now. You cannot see it again.</strong>
        <div className="flex items-center gap-2">
          <code className="min-w-0 flex-1 [overflow-wrap:anywhere] select-all">{secret.value}</code>
          <Button variant="outline" type="button" onClick={() => { void navigator.clipboard.writeText(secret.value).then(() => { setCopied(true); toast('Secret copied') }, () => toast.error('Could not copy the secret.')) }}><Copy className="size-4" /> {copied ? 'Copied' : 'Copy'}</Button>
        </div>
      </div> : <p className="m-0 text-muted-foreground">Check each request with its <code>webhook-signature</code> header, as the docs on webhooks describe.</p>}
    </SettingsCard>
    <SettingsCard title="Subscriptions" flush>
      <div className="flex flex-col divide-y">
        {webhooks.isPending ? <SettingsRow><LoadingText>Loading webhooks…</LoadingText></SettingsRow> : null}
        {webhooks.isError ? <SettingsRow role="alert">Webhooks could not be loaded.</SettingsRow> : null}
        {webhooks.data?.length === 0 ? <SettingsRow>No webhooks yet.</SettingsRow> : null}
        {webhooks.data?.map((webhook) => <div key={webhook.id} className="flex flex-col">
          <SettingsRow className="flex-nowrap">
            <RowIcon><Global /></RowIcon>
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">{webhook.name}{webhook.enabled ? null : <Badge variant="secondary">Off</Badge>}</span>
              <span className="truncate text-xs text-muted-foreground/70">{webhook.url}</span>
              <span className="text-xs text-muted-foreground/70">{webhook.event_types.join(', ')} · {webhook.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ') || 'No projects'}</span>
            </div>
            <Switch aria-label={`Send events to ${webhook.name}`} checked={webhook.enabled} disabled={updateWebhook.isPending} onCheckedChange={(enabled) => updateWebhook.mutate({ webhookId: webhook.id, body: { enabled } }, { onError: (error) => toast.error(failure(error, 'The webhook could not be changed.')) })} />
            <span className="flex gap-1">
              <Button variant="ghost" type="button" aria-expanded={openDeliveries === webhook.id} onClick={() => setOpenDeliveries(openDeliveries === webhook.id ? undefined : webhook.id)}>Deliveries</Button>
              <Button variant="ghost" type="button" onClick={() => setEditing(webhook)}>Edit</Button>
              <Button variant="ghost" type="button" disabled={rotateSecret.isPending} onClick={() => void rotate(webhook)}>Rotate secret</Button>
              <Button variant="ghost" type="button" disabled={deleteWebhook.isPending} onClick={async () => { if (await confirmAction({ title: `Delete ${webhook.name}?`, description: 'Pending deliveries are not sent, and the delivery log is removed.', confirmLabel: 'Delete', danger: true })) deleteWebhook.mutate(webhook.id, { onError: () => toast.error('The webhook could not be deleted.') }) }}>Delete</Button>
            </span>
          </SettingsRow>
          {openDeliveries === webhook.id ? <Deliveries workspaceId={workspace.id} webhookId={webhook.id} /> : null}
        </div>)}
      </div>
    </SettingsCard>
    {editing ? <WebhookDialog
      workspaceId={workspace.id}
      webhook={editing === 'new' ? undefined : editing}
      projects={projects}
      projectsLoading={projectsQuery.isPending}
      onClose={() => setEditing(undefined)}
      onCreated={(name, value) => showSecret(name, value)}
    /> : null}
  </>
}

function Deliveries({ workspaceId, webhookId }: { workspaceId: string; webhookId: string }) {
  const deliveries = useWebhookDeliveries(workspaceId, webhookId)
  const retry = useRetryWebhookDelivery(workspaceId, webhookId)
  return <div className="border-t bg-muted/30 px-4 py-3 text-xs">
    {deliveries.isPending ? <LoadingText>Loading deliveries…</LoadingText> : null}
    {deliveries.isError ? <p className="m-0" role="alert">Deliveries could not be loaded.</p> : null}
    {deliveries.data?.length === 0 ? <p className="m-0 text-muted-foreground">No deliveries in the last 30 days.</p> : null}
    {deliveries.data?.length ? <table className="w-full border-collapse">
      <thead className="text-left text-muted-foreground">
        <tr><th className="py-1 pr-3 font-medium">Event</th><th className="py-1 pr-3 font-medium">State</th><th className="py-1 pr-3 font-medium">Attempts</th><th className="py-1 pr-3 font-medium">Last answer</th><th className="py-1 pr-3 font-medium">Created</th><th /></tr>
      </thead>
      <tbody>
        {deliveries.data.map((delivery) => <DeliveryRow key={delivery.id} delivery={delivery} retrying={retry.isPending} onRetry={() => retry.mutate(delivery.id, { onError: () => toast.error('The delivery could not be sent again.') })} />)}
      </tbody>
    </table> : null}
  </div>
}

function DeliveryRow({ delivery, retrying, onRetry }: { delivery: WebhookDelivery; retrying: boolean; onRetry: () => void }) {
  const badge = STATE_BADGES[delivery.state] ?? { label: delivery.state, variant: 'secondary' as const }
  const answer = delivery.last_status != null ? `HTTP ${delivery.last_status}` : delivery.last_error ?? '—'
  return <tr className="border-t border-border/60">
    <td className="py-1.5 pr-3"><code>{delivery.event_type}</code></td>
    <td className="py-1.5 pr-3"><Badge variant={badge.variant}>{badge.label}</Badge></td>
    <td className="py-1.5 pr-3 tabular-nums">{delivery.attempt_count}</td>
    <td className="py-1.5 pr-3">{answer}</td>
    <td className="py-1.5 pr-3 text-muted-foreground">{relativeTime(delivery.created_at)}</td>
    <td className="py-1.5 text-right">{delivery.state === 'failed' || delivery.state === 'cancelled' ? <Button size="sm" variant="outline" type="button" disabled={retrying} onClick={onRetry}>Retry</Button> : null}</td>
  </tr>
}

function WebhookDialog({ workspaceId, webhook, projects, projectsLoading, onClose, onCreated }: {
  workspaceId: string
  webhook?: WebhookSubscription
  projects: { id: string; name: string }[]
  projectsLoading: boolean
  onClose: () => void
  onCreated: (name: string, secret: string) => void
}) {
  const createWebhook = useCreateWebhook(workspaceId)
  const updateWebhook = useUpdateWebhook(workspaceId)
  const [name, setName] = useState(webhook?.name ?? '')
  const [url, setUrl] = useState(webhook?.url ?? '')
  const [eventTypes, setEventTypes] = useState<WebhookEventType[]>(webhook?.event_types ?? EVENT_TYPES.map((event) => event.value))
  const [projectIds, setProjectIds] = useState<string[]>(webhook?.project_ids ?? [])
  const [enabled, setEnabled] = useState(webhook?.enabled ?? true)
  const [error, setError] = useState<string>()
  const pending = createWebhook.isPending || updateWebhook.isPending
  const ready = name.trim() !== '' && url.trim() !== '' && eventTypes.length > 0 && projectIds.length > 0

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (!ready) return
    const body = { name: name.trim(), url: url.trim(), event_types: eventTypes, project_ids: projectIds, enabled }
    setError(undefined)
    try {
      if (webhook) {
        await updateWebhook.mutateAsync({ webhookId: webhook.id, body })
      } else {
        const created = await createWebhook.mutateAsync(body)
        onCreated(created.name, created.signing_secret)
      }
      onClose()
    } catch (caught) {
      setError(failure(caught, webhook ? 'The webhook could not be saved.' : 'The webhook could not be created.'))
    }
  }

  return <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
    <DialogContent className="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{webhook ? `Edit ${webhook.name}` : 'Add webhook'}</DialogTitle>
        <DialogDescription>{webhook ? 'Changes apply to the next event.' : 'The signing secret is shown once, after you add the webhook.'}</DialogDescription>
      </DialogHeader>
      <form id="webhook-form" className="grid gap-4" onSubmit={(event) => void submit(event)}>
        <Field><FieldLabel htmlFor="webhook-name">Name <RequiredMark /></FieldLabel><Input id="webhook-name" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} placeholder="Billing sync" /></Field>
        <Field><FieldLabel htmlFor="webhook-url">URL <RequiredMark /></FieldLabel><Input id="webhook-url" type="url" value={url} maxLength={2048} onChange={(event) => setUrl(event.target.value)} placeholder="https://example.com/orbit-webhook" /></Field>
        <div role="group" aria-labelledby="webhook-events" className="grid gap-3">
          <span id="webhook-events" className="text-sm font-medium">Events <RequiredMark /></span>
          {EVENT_TYPES.map((option) => <Label key={option.value} className="flex items-center gap-2">
            <Checkbox checked={eventTypes.includes(option.value)} onCheckedChange={(checked) => setEventTypes(checked ? [...eventTypes, option.value] : eventTypes.filter((other) => other !== option.value))} />
            {option.label}
          </Label>)}
        </div>
        <div className="grid gap-3">
          <span className="text-sm font-medium">Projects <RequiredMark /></span>
          <ProjectPicker id="webhook-projects" projects={projects} selected={projectIds} onChange={setProjectIds} empty={projectsLoading ? 'Loading projects…' : projects.length ? undefined : 'This workspace has no active projects.'} />
        </div>
        <Label className="flex items-center gap-2"><Switch checked={enabled} onCheckedChange={setEnabled} /> Send events</Label>
        {error ? <p className="m-0 text-destructive" role="alert">{error}</p> : null}
      </form>
      <DialogFooter>
        <Button variant="outline" type="button" onClick={onClose}>Cancel</Button>
        <Button type="submit" form="webhook-form" disabled={pending || !ready}>{webhook ? 'Save' : 'Add webhook'}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
}
