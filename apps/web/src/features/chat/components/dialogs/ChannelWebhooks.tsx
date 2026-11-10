import { useId, useState } from 'react'
import { Add, Copy, Danger, Edit, Flash, Key, MoreH, Trash } from 'reicon-react'
import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Switch } from '@/components/ui/switch'
import type { CreatedWebhook } from '@/features/chat/api/client'
import {
  useCreateWebhook,
  useDeleteWebhook,
  useRotateWebhook,
  useSetWebhookInteractivity,
  useUpdateWebhook,
  useWebhookSigningSecret,
} from '@/features/chat/api/mutations'
import { useWebhookDeliveries, useWebhooks } from '@/features/chat/api/queries'
import type { ChannelWebhook, Conversation } from '@/features/chat/api/types'
import { BotAvatar } from '@/features/chat/components/messages/BotAvatar'
import { authorName, useChatPeople } from '@/features/chat/components/messages/people'
import { relativeAgo } from '@/features/chat/lib/time'
import { chatErrorMessage } from './channelLib'

const NAME_MAX = 80

function copy(text: string, done: string) {
  navigator.clipboard.writeText(text).then(
    () => toast.success(done),
    () => toast.error('Could not copy. Select the text and copy it.'),
  )
}

/** The curl command that posts a test message to `url`. */
function curlExample(url: string): string {
  return `curl -X POST -H 'Content-Type: application/json' \\\n  -d '{"text": "Deploy **finished** :rocket:"}' \\\n  '${url}'`
}

/**
 * The incoming webhooks of a channel: a secret URL that an external tool posts JSON to, so a message shows in the
 * channel as that tool. The URL holds the secret, so it shows only right after it is made or rotated.
 */
export function ChannelWebhooks({ conversation }: { conversation: Conversation }) {
  const webhooks = useWebhooks(conversation.id)
  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [shown, setShown] = useState<CreatedWebhook | null>(null)
  const rotate = useRotateWebhook()
  const remove = useDeleteWebhook()
  const list = webhooks.data ?? []

  const rotateUrl = async (webhook: ChannelWebhook) => {
    const confirmed = await confirmAction({
      title: `Make a new URL for ${webhook.name}?`,
      description: 'The old URL stops working now. Tools that use it must get the new one.',
      confirmLabel: 'Make new URL',
      danger: true,
    })
    if (!confirmed) return
    rotate.mutate(
      { conversationId: conversation.id, webhookId: webhook.id },
      { onSuccess: setShown, onError: (error) => toast.error(chatErrorMessage(error, 'Could not make a new URL. Try again.')) },
    )
  }

  const deleteWebhook = async (webhook: ChannelWebhook) => {
    const confirmed = await confirmAction({
      title: `Delete ${webhook.name}?`,
      description: 'Its URL stops working. The messages it posted stay.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!confirmed) return
    if (shown?.webhook.id === webhook.id) setShown(null)
    remove.mutate(
      { conversationId: conversation.id, webhookId: webhook.id },
      { onError: (error) => toast.error(chatErrorMessage(error, 'Could not delete the webhook. Try again.')) },
    )
  }

  return (
    <div data-slot="channel-webhooks" className="grid gap-4">
      <p className="text-sm text-muted-foreground">
        Incoming webhooks let tools such as CI, monitoring or ticketing post messages to #{conversation.name}. Each one has a secret URL that takes a
        JSON message.
      </p>

      {shown ? <WebhookUrl created={shown} onDone={() => setShown(null)} /> : null}

      {webhooks.isError ? (
        <p role="alert" className="text-sm text-destructive">
          {chatErrorMessage(webhooks.error, 'Could not load the webhooks. Try again.')}
        </p>
      ) : webhooks.isPending ? null : list.length === 0 && !creating ? (
        <div className="grid justify-items-start gap-3 rounded-lg border border-dashed p-4">
          <p className="text-sm text-muted-foreground">No incoming webhooks yet.</p>
          <Button type="button" size="sm" onClick={() => setCreating(true)}>
            <Add data-icon="inline-start" />
            New webhook
          </Button>
        </div>
      ) : (
        <ul className="grid gap-1">
          {list.map((webhook) =>
            editing === webhook.id ? (
              <li key={webhook.id}>
                <WebhookForm conversationId={conversation.id} webhook={webhook} onDone={() => setEditing(null)} />
              </li>
            ) : (
              <WebhookRow
                key={webhook.id}
                webhook={webhook}
                conversationId={conversation.id}
                onRename={() => setEditing(webhook.id)}
                onRotate={() => void rotateUrl(webhook)}
                onDelete={() => void deleteWebhook(webhook)}
              />
            ),
          )}
        </ul>
      )}

      {creating ? (
        <WebhookForm
          conversationId={conversation.id}
          onDone={(created) => {
            setCreating(false)
            if (created) setShown(created)
          }}
        />
      ) : list.length > 0 ? (
        <Button type="button" variant="outline" size="sm" className="justify-self-start" onClick={() => setCreating(true)}>
          <Add data-icon="inline-start" />
          New webhook
        </Button>
      ) : null}
    </div>
  )
}

function WebhookRow({
  webhook,
  conversationId,
  onRename,
  onRotate,
  onDelete,
}: {
  webhook: ChannelWebhook
  conversationId: string
  onRename: () => void
  onRotate: () => void
  onDelete: () => void
}) {
  const [interactivityOpen, setInteractivityOpen] = useState(false)
  const people = useChatPeople()
  const creator = authorName(people, webhook.createdBy, null, 'a former member')
  const used = webhook.lastUsedAt === null ? 'never used' : `last used ${relativeAgo(webhook.lastUsedAt)}`
  return (
    <li data-slot="channel-webhook" className="grid gap-2">
      <div className="flex items-center gap-3 rounded-md px-2 py-1.5 hover:bg-muted/50">
        <BotAvatar bot={webhook} size={32} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{webhook.name}</p>
          <p className="truncate text-xs text-muted-foreground">
            Made by {creator} · {used}
            {webhook.requestUrl ? ' · buttons on' : ''}
            {webhook.events ? ' · events on' : ''}
          </p>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label={`Options for ${webhook.name}`} />
            }
          >
            <MoreH />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={onRename}>
              <Edit />
              Edit
            </DropdownMenuItem>
            <DropdownMenuItem onClick={onRotate}>
              <Key />
              Make new URL
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setInteractivityOpen((open) => !open)}>
              <Flash />
              Interactivity
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onClick={onDelete}>
              <Trash />
              Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {interactivityOpen ? (
        <WebhookInteractivity webhook={webhook} conversationId={conversationId} onClose={() => setInteractivityOpen(false)} />
      ) : null}
    </li>
  )
}

/**
 * Where clicks on the webhook's buttons (and, with events on, members' messages) go, the secret that signs them, and the
 * last deliveries with their answers. The receiver checks `X-Slack-Signature` as for Slack, and answers through the
 * click's `response_url` or the webhook URL.
 */
function WebhookInteractivity({ webhook, conversationId, onClose }: { webhook: ChannelWebhook; conversationId: string; onClose: () => void }) {
  const [requestUrl, setRequestUrl] = useState(webhook.requestUrl ?? '')
  const [secret, setSecret] = useState<string | null>(null)
  const save = useSetWebhookInteractivity()
  const signingSecret = useWebhookSigningSecret()
  const deliveries = useWebhookDeliveries(conversationId, webhook.id, webhook.requestUrl !== null)
  const urlId = useId()
  const secretId = useId()
  const eventsId = useId()
  const changed = requestUrl.trim() !== (webhook.requestUrl ?? '')

  const submit = (url: string | null, events: boolean) =>
    save.mutate(
      { conversationId, webhookId: webhook.id, requestUrl: url, events: url !== null && events },
      {
        onSuccess: (next) => {
          setRequestUrl(next.requestUrl ?? '')
          toast.success(
            next.events
              ? 'Clicks and messages go to the request URL.'
              : next.requestUrl
                ? 'Buttons send clicks to the request URL.'
                : 'Buttons and events are off.',
          )
        },
      },
    )
  const loadSecret = async (rotate: boolean) => {
    if (rotate) {
      const confirmed = await confirmAction({
        title: 'Make a new signing secret?',
        description: 'Clicks are signed with the new secret at once. The receiver must check them with it.',
        confirmLabel: 'Make new secret',
        danger: true,
      })
      if (!confirmed) return
    }
    signingSecret.mutate(
      { conversationId, webhookId: webhook.id, rotate },
      { onSuccess: setSecret, onError: (error) => toast.error(chatErrorMessage(error, 'Could not read the signing secret.')) },
    )
  }

  return (
    <div data-slot="webhook-interactivity" className="grid gap-5 rounded-lg border p-4">
      <form
        onSubmit={(event) => {
          event.preventDefault()
          event.stopPropagation()
          if (requestUrl.trim()) submit(requestUrl.trim(), webhook.events)
        }}
      >
        <Field data-invalid={save.isError || undefined}>
          <FieldLabel htmlFor={urlId}>
            Request URL
            <InfoTip text="A click on a button without a link sends a signed block_actions payload here, as Slack does. Answer through its response_url to replace the message, delete it, or reply." />
          </FieldLabel>
          <div className="flex gap-2">
            <Input
              id={urlId}
              type="url"
              inputMode="url"
              placeholder="https://example.com/slack/actions"
              value={requestUrl}
              onChange={(event) => setRequestUrl(event.target.value)}
            />
            <Button type="submit" disabled={!requestUrl.trim() || !changed || save.isPending}>
              {save.isPending ? 'Saving…' : 'Save'}
            </Button>
          </div>
          {save.isError ? (
            <FieldError>{chatErrorMessage(save.error, 'The request URL was not saved.', { conflict: 'Use a public https URL.' })}</FieldError>
          ) : null}
        </Field>
      </form>

      {webhook.requestUrl ? (
        <>
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-center gap-1.5">
              <label htmlFor={eventsId} className="text-sm font-medium">
                Send messages as events
              </label>
              <InfoTip
                text={`Each member's message in this channel goes to the request URL as a signed event_callback (app_mention when it names @${webhook.name}). The webhook URL's secret can then also read the channel and its threads.`}
              />
            </div>
            <Switch
              id={eventsId}
              checked={webhook.events}
              disabled={save.isPending}
              onCheckedChange={(checked: boolean) => submit(webhook.requestUrl, checked)}
            />
          </div>

          <Field>
            <FieldLabel htmlFor={secretId}>Signing secret</FieldLabel>
            <div className="flex gap-2">
              <InputGroup>
                <InputGroupInput id={secretId} readOnly value={secret ?? '••••••••••••••••••••••••••••••••'} className="font-mono text-xs" />
                <InputGroupAddon align="inline-end">
                  {secret ? (
                    <InputGroupButton aria-label="Copy signing secret" size="icon-xs" onClick={() => copy(secret, 'Signing secret copied')}>
                      <Copy />
                    </InputGroupButton>
                  ) : (
                    <InputGroupButton size="xs" disabled={signingSecret.isPending} onClick={() => void loadSecret(false)}>
                      Reveal
                    </InputGroupButton>
                  )}
                </InputGroupAddon>
              </InputGroup>
              <Button type="button" variant="outline" disabled={signingSecret.isPending} onClick={() => void loadSecret(true)}>
                <Key data-icon="inline-start" />
                Make new
              </Button>
            </div>
          </Field>

          <div className="grid gap-2">
            <p className="text-sm font-medium">Last deliveries</p>
            {deliveries.data && deliveries.data.length > 0 ? (
              <ul className="grid gap-1 text-xs">
                {deliveries.data.map((delivery) => {
                  const good = delivery.status !== null && delivery.status >= 200 && delivery.status < 300
                  return (
                    <li key={delivery.id} className="flex items-center gap-2">
                      <span className={good ? 'text-emerald-500' : 'text-destructive'}>{delivery.status ?? delivery.error}</span>
                      <span className="truncate font-mono">{delivery.actionId}</span>
                      <span className="ml-auto shrink-0 text-muted-foreground">
                        {delivery.durationMs} ms · {relativeAgo(delivery.createdAt)}
                      </span>
                    </li>
                  )
                })}
              </ul>
            ) : (
              <p className="text-xs text-muted-foreground">Nothing sent yet.</p>
            )}
          </div>
        </>
      ) : null}

      <div className="flex items-center justify-between gap-2 border-t pt-4">
        {webhook.requestUrl ? (
          <Button type="button" variant="ghost" size="sm" className="text-destructive" disabled={save.isPending} onClick={() => submit(null, false)}>
            Remove request URL
          </Button>
        ) : (
          <span />
        )}
        <Button type="button" variant="ghost" size="sm" onClick={onClose}>
          Close
        </Button>
      </div>
    </div>
  )
}

/** Makes a webhook, or edits the name and icon of `webhook`. `onDone` gets the new webhook and its URL after a create. */
function WebhookForm({
  conversationId,
  webhook,
  onDone,
}: {
  conversationId: string
  webhook?: ChannelWebhook
  onDone: (created?: CreatedWebhook) => void
}) {
  const create = useCreateWebhook()
  const update = useUpdateWebhook()
  const [name, setName] = useState(webhook?.name ?? '')
  const [iconUrl, setIconUrl] = useState(webhook?.iconUrl ?? '')
  const nameId = useId()
  const iconId = useId()
  const pending = create.isPending || update.isPending
  const error = create.error ?? update.error
  const iconInvalid = iconUrl.trim() !== '' && !iconUrl.trim().startsWith('https://')

  const submit = async () => {
    const input = { conversationId, name: name.trim(), iconUrl: iconUrl.trim() || null }
    if (!input.name || iconInvalid || pending) return
    try {
      if (webhook) {
        await update.mutateAsync({ ...input, webhookId: webhook.id })
        onDone()
      } else {
        onDone(await create.mutateAsync(input))
      }
    } catch {
      // The error shows above the buttons.
    }
  }

  return (
    <form
      className="grid gap-3 rounded-lg border p-3"
      onSubmit={(event) => {
        event.preventDefault()
        // The dialog's own form must not get this submit.
        event.stopPropagation()
        void submit()
      }}
    >
      <Field>
        <FieldLabel htmlFor={nameId}>Name</FieldLabel>
        <Input id={nameId} autoFocus required maxLength={NAME_MAX} placeholder="CI" value={name} onChange={(event) => setName(event.target.value)} />
      </Field>
      <Field data-invalid={iconInvalid || undefined}>
        <FieldLabel htmlFor={iconId}>Icon URL (optional)</FieldLabel>
        <Input
          id={iconId}
          type="url"
          inputMode="url"
          placeholder="https://example.com/icon.png"
          value={iconUrl}
          aria-invalid={iconInvalid || undefined}
          onChange={(event) => setIconUrl(event.target.value)}
        />
        {iconInvalid ? <FieldError>Use an https:// image URL.</FieldError> : null}
      </Field>
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {chatErrorMessage(error, 'The webhook was not saved. Try again.', {
            conflict: 'A channel can have at most 10 incoming webhooks.',
            forbidden: 'Only the channel creator and workspace owners and admins can manage webhooks.',
          })}
        </p>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" size="sm" onClick={() => onDone()}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={!name.trim() || iconInvalid || pending}>
          {pending ? 'Saving…' : webhook ? 'Save' : 'Create webhook'}
        </Button>
      </div>
    </form>
  )
}

/** The URL of a webhook that was just made or rotated, with a curl example. It does not show again. */
function WebhookUrl({ created, onDone }: { created: CreatedWebhook; onDone: () => void }) {
  const urlId = useId()
  const example = curlExample(created.url)
  return (
    <div data-slot="webhook-url" className="grid gap-3 rounded-lg border p-3">
      <Field>
        <FieldLabel htmlFor={urlId}>URL of {created.webhook.name}</FieldLabel>
        <InputGroup>
          <InputGroupInput id={urlId} readOnly value={created.url} className="font-mono text-xs" onFocus={(event) => event.target.select()} />
          <InputGroupAddon align="inline-end">
            <InputGroupButton aria-label="Copy URL" size="icon-xs" onClick={() => copy(created.url, 'URL copied')}>
              <Copy />
            </InputGroupButton>
          </InputGroupAddon>
        </InputGroup>
      </Field>
      <p className="flex items-start gap-2 text-xs text-muted-foreground">
        <Danger aria-hidden="true" className="mt-px size-4 shrink-0 text-amber-500" />
        Copy it now: Orbit shows this URL only once. Anyone who has it can post to this channel.
      </p>
      <div className="grid gap-1.5">
        <div className="flex items-center justify-between">
          <span className="text-xs font-medium">Try it</span>
          <Button type="button" variant="ghost" size="xs" onClick={() => copy(example, 'Command copied')}>
            <Copy data-icon="inline-start" />
            Copy
          </Button>
        </div>
        <pre className="overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre">{example}</pre>
      </div>
      <Button type="button" size="sm" variant="outline" className="justify-self-end" onClick={onDone}>
        Done
      </Button>
    </div>
  )
}
