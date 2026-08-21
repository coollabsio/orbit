// The chat reference-style modals: CreateChannel / CreateCategory / EditChannel / ConfirmDelete
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Modal } from '../../../components/ui/Modal'
import {
  createChannel,
  createChatCategory,
  deleteChannel,
  deleteChatCategory,
  updateChannel,
} from '../../../mock/actions'
import type { Channel } from '../../../mock/types'

export type ChannelModalState =
  | { kind: 'create-channel'; categoryId: string; categoryName: string }
  | { kind: 'create-category' }
  | { kind: 'edit-channel'; channel: Channel }
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

function ModalShell({
  title,
  description,
  children,
  onClose,
}: {
  title: string
  description?: string
  children: React.ReactNode
  onClose: () => void
}) {
  return (
    <Modal title={title} description={description} onClose={onClose} maxWidth={480}>
      <div className="fc-modal-form">{children}</div>
    </Modal>
  )
}

function CreateChannelModal({ categoryId, categoryName, onClose }: { categoryId: string; categoryName: string; onClose: () => void }) {
  const [name, setName] = useState('')
  const navigate = useNavigate()

  function commit() {
    const trimmed = name.trim().toLowerCase().replace(/\s+/g, '-')
    if (!trimmed) return
    const id = createChannel(categoryId, trimmed)
    onClose()
    navigate(`/chat/${id}`)
  }

  return (
    <ModalShell title="Create channel" description={`Add a channel to ${categoryName}.`} onClose={onClose}>
      <label className="fc-modal-label">Channel name</label>
      <input
        className="fc-modal-input"
        value={name}
        placeholder="new-channel"
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
        }}
      />
      <div className="fc-modal-footer">
        <button className="fc-modal-button" data-variant="secondary" onClick={onClose}>
          Cancel
        </button>
        <button className="fc-modal-button" data-variant="primary" disabled={!name.trim()} onClick={commit}>
          Create Channel
        </button>
      </div>
    </ModalShell>
  )
}

function CreateCategoryModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('')

  function commit() {
    if (!name.trim()) return
    createChatCategory(name.trim())
    onClose()
  }

  return (
    <ModalShell title="Create category" description="Organize related channels into a new category." onClose={onClose}>
      <label className="fc-modal-label">Category name</label>
      <input
        className="fc-modal-input"
        value={name}
        placeholder="New Category"
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
        }}
      />
      <div className="fc-modal-footer">
        <button className="fc-modal-button" data-variant="secondary" onClick={onClose}>
          Cancel
        </button>
        <button className="fc-modal-button" data-variant="primary" disabled={!name.trim()} onClick={commit}>
          Create Category
        </button>
      </div>
    </ModalShell>
  )
}

function EditChannelModal({ channel, onClose }: { channel: Channel; onClose: () => void }) {
  const [name, setName] = useState(channel.name)
  const [topic, setTopic] = useState(channel.description)

  function commit() {
    const trimmed = name.trim().toLowerCase().replace(/\s+/g, '-')
    if (!trimmed) return
    updateChannel(channel.id, { name: trimmed, description: topic.trim() })
    onClose()
  }

  return (
    <ModalShell title="Edit channel" description={`Update #${channel.name}.`} onClose={onClose}>
      <label className="fc-modal-label">Channel name</label>
      <input
        className="fc-modal-input"
        value={name}
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
        }}
      />
      <label className="fc-modal-label">Topic</label>
      <input
        className="fc-modal-input"
        value={topic}
        placeholder="What is this channel about?"
        onChange={(e) => setTopic(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
        }}
      />
      <div className="fc-modal-footer">
        <button className="fc-modal-button" data-variant="secondary" onClick={onClose}>
          Cancel
        </button>
        <button className="fc-modal-button" data-variant="primary" disabled={!name.trim()} onClick={commit}>
          Save
        </button>
      </div>
    </ModalShell>
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
    <ModalShell title={title} description={description} onClose={onClose}>
      <div className="fc-modal-footer">
        <button className="fc-modal-button" data-variant="secondary" onClick={onClose}>
          Cancel
        </button>
        <button
          className="fc-modal-button"
          data-variant="danger"
          onClick={() => {
            onConfirm()
            if (navigateAwayFrom && navigateAwayFrom === activeChannelId) navigate('/chat')
          }}
        >
          Delete
        </button>
      </div>
    </ModalShell>
  )
}
