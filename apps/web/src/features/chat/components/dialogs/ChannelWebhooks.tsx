import { useId, useState } from 'react'
import { Add, Copy, Danger, Edit, Key, MoreH, Trash } from 'reicon-react'
import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import type { CreatedWebhook } from '@/features/chat/api/client'
import { useCreateWebhook, useDeleteWebhook, useRotateWebhook, useUpdateWebhook } from '@/features/chat/api/mutations'
import { useWebhooks } from '@/features/chat/api/queries'
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
        Incoming webhooks let tools such as CI, monitoring or ticketing post messages to #{conversation.name}. Each one has a secret URL that takes a JSON
        message.
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
              <WebhookRow key={webhook.id} webhook={webhook} onRename={() => setEditing(webhook.id)} onRotate={() => void rotateUrl(webhook)} onDelete={() => void deleteWebhook(webhook)} />
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

function WebhookRow({ webhook, onRename, onRotate, onDelete }: { webhook: ChannelWebhook; onRename: () => void; onRotate: () => void; onDelete: () => void }) {
  const people = useChatPeople()
  const creator = authorName(people, webhook.createdBy, null, 'a former member')
  const used = webhook.lastUsedAt === null ? 'never used' : `last used ${relativeAgo(webhook.lastUsedAt)}`
  return (
    <li data-slot="channel-webhook" className="flex items-center gap-3 rounded-md px-2 py-1.5 hover:bg-muted/50">
      <BotAvatar bot={webhook} size={32} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium">{webhook.name}</p>
        <p className="truncate text-xs text-muted-foreground">
          Made by {creator} · {used}
        </p>
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground" aria-label={`Options for ${webhook.name}`} />}>
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
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onClick={onDelete}>
            <Trash />
            Delete
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  )
}

/** Makes a webhook, or edits the name and icon of `webhook`. `onDone` gets the new webhook and its URL after a create. */
function WebhookForm({ conversationId, webhook, onDone }: { conversationId: string; webhook?: ChannelWebhook; onDone: (created?: CreatedWebhook) => void }) {
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
