// Chat settings › Emoji: upload custom emojis usable across the whole app as :name:.
import { useRef, useState } from 'react'
import { Trash } from 'reicon-react'
import { createCustomEmoji, deleteCustomEmoji, renameCustomEmoji } from '../../../mock/actions'
import { useAppState } from '../../../mock/store'
import type { CustomEmoji } from '../../../mock/types'
import { ConfirmDeleteModal } from '../components/ChannelModals'
import './server.css'

const MAX_EMOJI_BYTES = 256 * 1024

/** File name → shortcode: lowercase, [a-z0-9_], no extension. */
function slugify(fileName: string): string {
  return fileName
    .replace(/\.[^.]+$/, '')
    .toLowerCase()
    .replace(/[^a-z0-9_+-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 32)
}

export function EmojiTab() {
  const state = useAppState()
  const fileInput = useRef<HTMLInputElement>(null)
  const [error, setError] = useState('')
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [deleteTarget, setDeleteTarget] = useState<CustomEmoji | null>(null)

  const upload = (files: FileList | null) => {
    if (!files) return
    setError('')
    for (const file of Array.from(files)) {
      if (!file.type.startsWith('image/')) {
        setError('Emojis must be images.')
        continue
      }
      if (file.size > MAX_EMOJI_BYTES) {
        setError('Emojis must be 256KB or smaller.')
        continue
      }
      createCustomEmoji(slugify(file.name) || 'emoji', URL.createObjectURL(file))
    }
  }

  const commitName = (emoji: CustomEmoji) => {
    const draft = drafts[emoji.id]
    if (draft === undefined) return
    const name = slugify(`${draft}.x`)
    if (name && name !== emoji.name) renameCustomEmoji(emoji.id, name)
    setDrafts((prev) => {
      const { [emoji.id]: _discard, ...rest } = prev
      return rest
    })
  }

  return (
    <div className="fs-page" style={{ gap: 32 }}>
      <div className="fs-webhooks-head">
        <div>
          <h2 className="fs-title">Emoji</h2>
          <p className="fs-subtitle" data-strong style={{ marginTop: 12, maxWidth: 576, lineHeight: '24px' }}>
            Custom emojis work everywhere in the app: type :name: in chat, comments and docs, or pick them
            from any emoji panel.
          </p>
        </div>
        <button type="button" className="fs-btn" data-variant="primary" onClick={() => fileInput.current?.click()}>
          Upload Emoji
        </button>
        <input
          ref={fileInput}
          type="file"
          accept="image/*"
          multiple
          hidden
          aria-label="Upload emoji images"
          onChange={(e) => {
            upload(e.target.files)
            e.target.value = ''
          }}
        />
      </div>

      {error ? <p className="fs-error">{error}</p> : null}

      {state.customEmojis.length === 0 ? (
        <p className="wh-empty">No custom emojis yet.</p>
      ) : (
        <div className="em-list">
          <div className="em-head-row">
            <span>Emoji</span>
            <span>Name</span>
            <span />
          </div>
          {state.customEmojis.map((emoji) => (
            <div key={emoji.id} className="em-row">
              <img className="em-image" src={emoji.url} alt={`:${emoji.name}:`} />
              <div className="em-name">
                <input
                  className="input em-name-input"
                  value={drafts[emoji.id] ?? emoji.name}
                  aria-label={`Emoji name ${emoji.name}`}
                  onChange={(e) => setDrafts((prev) => ({ ...prev, [emoji.id]: e.target.value }))}
                  onBlur={() => commitName(emoji)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') e.currentTarget.blur()
                  }}
                />
              </div>
              <button
                type="button"
                className="icon-button em-delete"
                aria-label={`Delete :${emoji.name}:`}
                title="Delete"
                onClick={() => setDeleteTarget(emoji)}
              >
                <Trash size={15} />
              </button>
            </div>
          ))}
        </div>
      )}

      {deleteTarget ? (
        <ConfirmDeleteModal
          title={`Delete :${deleteTarget.name}:?`}
          description="Messages that use this emoji will show the plain :name: text instead."
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => {
            deleteCustomEmoji(deleteTarget.id)
            setDeleteTarget(null)
          }}
        />
      ) : null}
    </div>
  )
}
