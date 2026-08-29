// Server settings › Webhooks: accordion rows (icon, name, created; expanded: icon upload,
// name, channel, copy URL, delete).
import { useState } from 'react'
import { ArrowRight2, Copy, TickCircle, Trash } from 'reicon-react'
import { Listbox } from '../../../components/ui/Listbox'
import { WebhookIcon } from '../../../components/ui/WebhookIcon'
import { createWebhook, deleteWebhook, updateWebhook } from '../../../mock/actions'
import { useAppState } from '../../../mock/store'
import type { Webhook } from '../../../mock/types'
import { ConfirmDeleteModal } from '../components/ChannelModals'
import { channelLabel, webhookUrl } from './webhookLib'
import './server.css'
import './webhooks.css'

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
    <div className="fs-page" style={{ gap: 32 }}>
      <div className="fs-webhooks-head">
        <div>
          <h2 className="fs-title">Webhooks</h2>
          <p className="fs-subtitle" data-strong style={{ marginTop: 12, maxWidth: 576, lineHeight: '24px' }}>
            Webhooks post messages from other apps and websites into this server.
          </p>
        </div>
        <button type="button" className="fs-btn" data-variant="primary" onClick={handleCreate}>
          New Webhook
        </button>
      </div>

      {error ? <p className="fs-error">{error}</p> : null}

      {state.webhooks.length === 0 ? (
        <p className="wh-empty">No webhooks yet.</p>
      ) : (
        <div className="wh-list">
          {state.webhooks.map((wh) => {
            const isExpanded = expandedId === wh.id
            const isUploading = uploadingId === wh.id
            return (
              <div key={wh.id} className="wh-row">
                <button type="button" className="wh-row-header" onClick={() => setExpandedId(isExpanded ? null : wh.id)}>
                  <span className="wh-avatar">{wh.iconUrl ? <img src={wh.iconUrl} alt="" /> : <WebhookIcon size={20} />}</span>
                  <span className="wh-row-text">
                    <span className="wh-row-name">{wh.name}</span>
                    <span className="wh-row-meta">{formatCreatedAt(wh.createdAt)}</span>
                  </span>
                  <ArrowRight2 size={14} className="wh-chevron" data-open={isExpanded || undefined} />
                </button>

                {isExpanded ? (
                  <>
                    <div className="wh-divider" />
                    <div className="wh-body">
                      <div>
                        <p className="wh-label">Icon</p>
                        <label className="wh-icon-upload">
                          <input
                            type="file"
                            accept="image/*"
                            disabled={isUploading}
                            aria-label="Upload webhook icon"
                            onChange={(event) => {
                              void handleUploadIcon(wh.id, event.target.files?.[0])
                              event.currentTarget.value = ''
                            }}
                          />
                          {wh.iconUrl ? <img src={wh.iconUrl} alt="" /> : <WebhookIcon size={32} />}
                          {isUploading ? <span className="wh-icon-busy">...</span> : null}
                        </label>
                        <p className="wh-icon-hint">
                          <span>1000x1000</span>
                          <span>5MB max</span>
                        </p>
                      </div>
                      <div className="wh-fields">
                        <div className="wh-grid">
                          <label className="wh-field">
                            <span className="wh-label">Name</span>
                            <input
                              type="text"
                              className="fs-input"
                              value={nameDrafts[wh.id] ?? wh.name}
                              onChange={(e) => setNameDrafts((prev) => ({ ...prev, [wh.id]: e.target.value }))}
                              onBlur={() => commitName(wh)}
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') e.currentTarget.blur()
                              }}
                            />
                          </label>
                          <div className="wh-field">
                            <span className="wh-label">Channel</span>
                            <Listbox
                              value={wh.channelId}
                              options={state.channels.map((c) => ({ value: c.id, label: channelLabel(state.channels, c.id) }))}
                              onChange={(channelId) => updateWebhook(wh.id, { channelId })}
                            />
                          </div>
                        </div>
                        <div className="wh-actions">
                          <button type="button" className="fs-btn" data-variant="secondary" onClick={() => handleCopyUrl(wh)}>
                            {copiedId === wh.id ? <TickCircle size={16} /> : <Copy size={16} />}
                            {copiedId === wh.id ? 'Copied' : 'Copy Webhook URL'}
                          </button>
                          <button type="button" className="fs-btn wh-delete" onClick={() => setDeleteTarget(wh)}>
                            <Trash size={16} />
                            Delete Webhook
                          </button>
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
