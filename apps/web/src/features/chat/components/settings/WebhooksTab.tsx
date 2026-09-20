// Server settings › Webhooks: accordion rows (icon, name, created; expanded: icon upload,
// name, channel, copy URL, delete).
import { useState } from 'react'
import { Check, ChevronRight, Copy, Trash as Trash2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { WebhookIcon } from '@/components/common/icons/WebhookIcon'
import { createWebhook, deleteWebhook, updateWebhook } from '@/mock/actions'
import { useAppState } from '@/mock/store'
import type { Webhook } from '@/mock/types'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { channelLabel, webhookUrl } from '@/features/chat/webhookLib'

const MAX_WEBHOOK_ICON_BYTES = 5 * 1024 * 1024
const MAX_WEBHOOK_ICON_DIMENSION = 1000

function getImageDimensions(file: File): Promise<{ width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file)
    const image = new Image()
    image.onload = () => {
      URL.revokeObjectURL(url)
      resolve({ width: image.naturalWidth, height: image.naturalHeight })
    }
    image.onerror = () => {
      URL.revokeObjectURL(url)
      reject(new Error('Could not read image dimensions'))
    }
    image.src = url
  })
}

function formatCreatedAt(iso: string): string {
  return `Created on ${new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })}`
}

export function WebhooksTab() {
  const state = useAppState()
  const [error, setError] = useState('')
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [nameDrafts, setNameDrafts] = useState<Record<string, string>>({})
  const [uploadingId, setUploadingId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<Webhook | null>(null)
  const channelItems = state.channels.map((c) => ({ value: c.id, label: channelLabel(state.channels, c.id) }))

  function handleCreate() {
    const firstChannel = state.channels[0]
    if (!firstChannel) {
      setError('Create a channel before adding a webhook.')
      return
    }
    setError('')
    const webhook = createWebhook(firstChannel.id)
    setExpandedId(webhook.id)
  }

  function commitName(webhook: Webhook) {
    const draft = nameDrafts[webhook.id]
    if (draft === undefined) return
    const name = draft.trim()
    if (!name) setError('Webhook name cannot be empty.')
    else if (name !== webhook.name) {
      setError('')
      updateWebhook(webhook.id, { name })
    }
    setNameDrafts((prev) => {
      const { [webhook.id]: _discard, ...rest } = prev
      return rest
    })
  }

  async function handleUploadIcon(id: string, file: File | undefined) {
    if (!file) return
    if (file.size > MAX_WEBHOOK_ICON_BYTES) {
      setError('Webhook icon must be 5MB or smaller.')
      return
    }
    setUploadingId(id)
    setError('')
    try {
      const dimensions = await getImageDimensions(file)
      if (dimensions.width > MAX_WEBHOOK_ICON_DIMENSION || dimensions.height > MAX_WEBHOOK_ICON_DIMENSION) {
        setError('Webhook icon must be 1000x1000 or smaller.')
        return
      }
      updateWebhook(id, { iconUrl: URL.createObjectURL(file) })
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to upload webhook icon')
    } finally {
      setUploadingId(null)
    }
  }

  function handleCopyUrl(webhook: Webhook) {
    void navigator.clipboard.writeText(webhookUrl(webhook)).then(() => {
      setCopiedId(webhook.id)
      setTimeout(() => setCopiedId(null), 2000)
    })
  }

  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-col items-start gap-4 border-b border-border pb-7">
        <div>
          <h2 className="text-xl leading-7 font-semibold text-foreground">Webhooks</h2>
          <p className="mt-3 max-w-[576px] text-sm leading-6 text-foreground">
            Webhooks post messages from other apps and websites into this server.
          </p>
        </div>
        <Button size="lg" onClick={handleCreate}>
          New Webhook
        </Button>
      </div>

      {error ? <p className="rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p> : null}

      {state.webhooks.length === 0 ? (
        <p className="py-8 text-sm text-muted-foreground">No webhooks yet.</p>
      ) : (
        <div className="flex flex-col gap-4">
          {state.webhooks.map((wh) => {
            const isExpanded = expandedId === wh.id
            const isUploading = uploadingId === wh.id
            return (
              <div key={wh.id} className="overflow-hidden rounded-lg border border-border bg-background">
                <Button
                  variant="ghost"
                  className="flex h-auto w-full justify-start gap-3.5 rounded-none border-0 px-4 py-3.5 text-left font-normal whitespace-normal hover:bg-foreground/[0.02] active:not-aria-[haspopup]:translate-y-0 dark:hover:bg-foreground/[0.02]"
                  onClick={() => setExpandedId(isExpanded ? null : wh.id)}
                >
                  <span className="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-full bg-amber-500/15 text-amber-400">
                    {wh.iconUrl ? <img className="size-full object-cover" src={wh.iconUrl} alt="" /> : <WebhookIcon size={20} />}
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-sm font-semibold text-foreground">{wh.name}</span>
                    <span className="mt-0.5 text-xs font-medium text-muted-foreground">{formatCreatedAt(wh.createdAt)}</span>
                  </span>
                  <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/55 transition-transform data-[open]:rotate-90" data-open={isExpanded || undefined} />
                </Button>

                {isExpanded ? (
                  <>
                    <Separator className="mx-4 data-horizontal:w-auto" />
                    <div className="grid grid-cols-[104px_minmax(0,1fr)] gap-5 p-4 max-[599px]:grid-cols-[minmax(0,1fr)]">
                      <div>
                        <p className="mb-2 text-sm font-semibold text-foreground">Icon</p>
                        <label className="relative flex size-16 cursor-pointer items-center justify-center overflow-hidden rounded-full bg-amber-500/15 text-amber-400 transition-opacity hover:opacity-85">
                          <input
                            type="file"
                            accept="image/*"
                            className="absolute inset-0 cursor-pointer opacity-0"
                            disabled={isUploading}
                            aria-label="Upload webhook icon"
                            onChange={(event) => {
                              void handleUploadIcon(wh.id, event.target.files?.[0])
                              event.currentTarget.value = ''
                            }}
                          />
                          {wh.iconUrl ? <img className="size-full object-cover" src={wh.iconUrl} alt="" /> : <WebhookIcon size={32} />}
                          {isUploading ? <span className="absolute inset-0 flex items-center justify-center bg-background/55 text-xs font-semibold text-foreground">...</span> : null}
                        </label>
                        <p className="mt-2 w-16 text-center text-[11px] font-medium leading-4 text-muted-foreground">
                          <span className="block">1000x1000</span>
                          <span className="block">5MB max</span>
                        </p>
                      </div>
                      <div className="flex min-w-0 flex-col gap-5">
                        <div className="grid grid-cols-2 gap-4 max-[599px]:grid-cols-[minmax(0,1fr)]">
                          <Label className="block min-w-0">
                            <span className="mb-2 block text-sm font-semibold text-foreground">Name</span>
                            <Input
                              type="text"
                              className="px-3"
                              value={nameDrafts[wh.id] ?? wh.name}
                              onChange={(e) => setNameDrafts((prev) => ({ ...prev, [wh.id]: e.target.value }))}
                              onBlur={() => commitName(wh)}
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') e.currentTarget.blur()
                              }}
                            />
                          </Label>
                          <div className="block min-w-0">
                            <Label htmlFor={`webhook-channel-${wh.id}`} className="mb-2 block text-sm font-semibold text-foreground">
                              Channel
                            </Label>
                            <Select
                              items={channelItems}
                              value={wh.channelId}
                              onValueChange={(channelId) => {
                                if (channelId) updateWebhook(wh.id, { channelId })
                              }}
                            >
                              <SelectTrigger
                                id={`webhook-channel-${wh.id}`}
                                aria-label="Channel"
                                className="w-full justify-between gap-2 rounded-md border-input bg-background px-3 font-normal shadow-xs data-[size=default]:h-9 dark:bg-background dark:hover:bg-muted"
                              >
                                <SelectValue placeholder="Select…" />
                              </SelectTrigger>
                              <SelectContent align="start" alignItemWithTrigger={false}>
                                {channelItems.map((item) => (
                                  <SelectItem key={item.value} value={item.value}>
                                    {item.label}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>
                        </div>
                        <div className="flex flex-wrap gap-3 border-t border-border pt-4">
                          <Button variant="secondary" size="lg" onClick={() => handleCopyUrl(wh)}>
                            {copiedId === wh.id ? <Check className="size-4" /> : <Copy className="size-4" />}
                            {copiedId === wh.id ? 'Copied' : 'Copy Webhook URL'}
                          </Button>
                          <Button variant="destructive" size="lg" onClick={() => setDeleteTarget(wh)}>
                            <Trash2 className="size-4" />
                            Delete Webhook
                          </Button>
                        </div>
                      </div>
                    </div>
                  </>
                ) : null}
              </div>
            )
          })}
        </div>
      )}

      {deleteTarget ? (
        <ConfirmDeleteModal
          title="Delete webhook?"
          description={`This will permanently delete ${deleteTarget.name}. Existing webhook URLs for it will stop working.`}
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => {
            deleteWebhook(deleteTarget.id)
            setExpandedId((current) => (current === deleteTarget.id ? null : current))
            setDeleteTarget(null)
          }}
        />
      ) : null}
    </div>
  )
}
