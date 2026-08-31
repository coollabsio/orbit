// the chat reference modals (Create/Edit Channel, Create/Edit Category, ConfirmDelete) rendered
// with the Coolify modal shell and form controls.
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { SmileCircle } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import { Emoji } from '../../../components/ui/Emoji'
import { EmojiPicker } from '../../../components/ui/EmojiPicker'
import { Modal } from '../../../components/ui/Modal'
import {
  createChannel,
  createChatCategory,
  deleteChannel,
  deleteChatCategory,
  updateChannel,
  updateChatCategory,
} from '../../../mock/actions'
import type { Channel, ChatCategory } from '../../../mock/types'

export type ChannelModalState =
  | { kind: 'create-channel'; categoryId: string; categoryName: string }
  | { kind: 'create-category' }
  | { kind: 'edit-channel'; channel: Channel }
  | { kind: 'edit-category'; category: ChatCategory }
  | { kind: 'delete-channel'; channel: Channel }
  | { kind: 'delete-category'; id: string; name: string }
  | null

interface ChannelModalsProps {
  modal: ChannelModalState
  onClose: () => void
  activeChannelId: string | null
}

export function ChannelModals({ modal, onClose, activeChannelId }: ChannelModalsProps) {
  if (!modal) return null
  switch (modal.kind) {
    case 'create-channel':
      return <CreateChannelModal categoryId={modal.categoryId} categoryName={modal.categoryName} onClose={onClose} />
    case 'create-category':
      return <CreateCategoryModal onClose={onClose} />
    case 'edit-channel':
      return <EditChannelModal channel={modal.channel} onClose={onClose} />
    case 'edit-category':
      return <EditCategoryModal category={modal.category} onClose={onClose} />
    case 'delete-channel':
      return (
        <ConfirmDeleteModal
          title="Delete channel?"
          description={`This will permanently delete #${modal.channel.name} and its messages.`}
          onClose={onClose}
          onConfirm={() => {
            deleteChannel(modal.channel.id)
            onClose()
          }}
          navigateAwayFrom={modal.channel.id}
          activeChannelId={activeChannelId}
        />
      )
    case 'delete-category':
      return (
        <ConfirmDeleteModal
          title="Delete category?"
          description={`This will permanently delete ${modal.name} and all channels inside it.`}
          onClose={onClose}
          onConfirm={() => {
            deleteChatCategory(modal.id)
            onClose()
          }}
          navigateAwayFrom={null}
          activeChannelId={activeChannelId}
        />
      )
  }
}

function slugChannelName(name: string) {
  return name.trim().toLowerCase().replace(/\s+/g, '-')
}

/** Emoji tile that opens the picker panel (replaces the old text input + "Focus input"). */
function EmojiSelect({ value, onChange, label = 'Emoji' }: { value: string; onChange: (v: string) => void; label?: string }) {
  return (
    <div className="settings-field">
      <span className="field-label">
        {label} <span className="text-muted">(optional)</span>
      </span>
      <Dropdown
        className="emoji-dropdown"
        trigger={() => (
          <button type="button" className="emoji-tile" aria-label={value.trim() ? `${label}: ${value}` : `Set ${label.toLowerCase()}`}>
            {value.trim() ? <Emoji value={value} size={20} /> : <SmileCircle size={18} />}
          </button>
        )}
      >
        {(close) => (
          <EmojiPicker
            onPick={(emoji) => {
              onChange(emoji)
              close()
            }}
            onRemove={
              value.trim()
                ? () => {
                    onChange('')
                    close()
                  }
                : undefined
            }
          />
        )}
      </Dropdown>
    </div>
  )
}

function FormFields({ children }: { children: React.ReactNode }) {
  return <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>{children}</div>
}

function CreateChannelModal({ categoryId, categoryName, onClose }: { categoryId: string; categoryName: string; onClose: () => void }) {
  const [name, setName] = useState('')
  const [emoji, setEmoji] = useState('')
  const navigate = useNavigate()

  function commit(e: React.FormEvent) {
    e.preventDefault()
    const slug = slugChannelName(name)
    if (!slug) return
    const id = createChannel(categoryId, slug)
    if (emoji.trim()) updateChannel(id, { emoji: emoji.trim() })
    onClose()
    navigate(`/chat/${id}`)
  }

  return (
    <Modal title="Create Channel" description={`in ${categoryName}`} onClose={onClose} maxWidth={448}>
      <form onSubmit={commit}>
        <FormFields>
          <div className="settings-field">
            <label className="field-label" htmlFor="channel-name">
              Channel Name <span className="field-required">*</span>
            </label>
            <input
              id="channel-name"
              className="input"
              value={name}
              placeholder="general"
              autoFocus
              required
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Channel emoji" />
        </FormFields>
        <div className="modal-footer">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={!name.trim()}>
            Create Channel
          </button>
        </div>
      </form>
    </Modal>
  )
}

function CreateCategoryModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('')
  const [emoji, setEmoji] = useState('')

  function commit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    createChatCategory(name.trim(), emoji.trim())
    onClose()
  }

  return (
    <Modal title="Create Category" description="Organize your channels into groups" onClose={onClose} maxWidth={448}>
      <form onSubmit={commit}>
        <FormFields>
          <div className="settings-field">
            <label className="field-label" htmlFor="category-name">
              Category Name <span className="field-required">*</span>
            </label>
            <input
              id="category-name"
              className="input"
              value={name}
              placeholder="New Category"
              autoFocus
              required
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Category emoji" />
        </FormFields>
        <div className="modal-footer">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={!name.trim()}>
            Create Category
          </button>
        </div>
      </form>
    </Modal>
  )
}

function EditChannelModal({ channel, onClose }: { channel: Channel; onClose: () => void }) {
  const [name, setName] = useState(channel.name)
  const [topic, setTopic] = useState(channel.description)
  const [emoji, setEmoji] = useState(channel.emoji ?? '')

  function commit(e: React.FormEvent) {
    e.preventDefault()
    const slug = slugChannelName(name)
    if (!slug) return
    updateChannel(channel.id, { name: slug, description: topic.trim(), emoji: emoji.trim() })
    onClose()
  }

  return (
    <Modal title="Edit Channel" onClose={onClose} maxWidth={448}>
      <form onSubmit={commit}>
        <FormFields>
          <div className="settings-field">
            <label className="field-label" htmlFor="edit-channel-name">
              Channel Name <span className="field-required">*</span>
            </label>
            <input
              id="edit-channel-name"
              className="input"
              value={name}
              placeholder="general"
              autoFocus
              required
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="settings-field">
            <label className="field-label" htmlFor="edit-channel-topic">
              Topic <span className="text-muted">(optional)</span>
            </label>
            <input
              id="edit-channel-topic"
              className="input"
              value={topic}
              placeholder="What's this channel about?"
              onChange={(e) => setTopic(e.target.value)}
            />
          </div>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Channel emoji" />
        </FormFields>
        <div className="modal-footer">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={!name.trim()}>
            Save Changes
          </button>
        </div>
      </form>
    </Modal>
  )
}

function EditCategoryModal({ category, onClose }: { category: ChatCategory; onClose: () => void }) {
  const [name, setName] = useState(category.name)
  const [emoji, setEmoji] = useState(category.emoji ?? '')

  function commit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    updateChatCategory(category.id, { name: name.trim(), emoji: emoji.trim() })
    onClose()
  }

  return (
    <Modal title="Edit Category" onClose={onClose} maxWidth={448}>
      <form onSubmit={commit}>
        <FormFields>
          <div className="settings-field">
            <label className="field-label" htmlFor="edit-category-name">
              Category Name <span className="field-required">*</span>
            </label>
            <input
              id="edit-category-name"
              className="input"
              value={name}
              placeholder="New Category"
              autoFocus
              required
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <EmojiSelect value={emoji} onChange={setEmoji} label="Category emoji" />
        </FormFields>
        <div className="modal-footer">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={!name.trim()}>
            Save Changes
          </button>
        </div>
      </form>
    </Modal>
  )
}

export function ConfirmDeleteModal({
  title,
  description,
  onClose,
  onConfirm,
  navigateAwayFrom,
  activeChannelId,
}: {
  title: string
  description: string
  onClose: () => void
  onConfirm: () => void
  navigateAwayFrom?: string | null
  activeChannelId?: string | null
}) {
  const navigate = useNavigate()
  return (
    <Modal title={title} onClose={onClose} maxWidth={384}>
      <p className="fc-confirm-text">{description}</p>
      <div className="modal-footer">
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="button button-danger"
          onClick={() => {
            onConfirm()
            if (navigateAwayFrom && navigateAwayFrom === activeChannelId) navigate('/chat')
          }}
        >
          Delete
        </button>
      </div>
    </Modal>
  )
}
