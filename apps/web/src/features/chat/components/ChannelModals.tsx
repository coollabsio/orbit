// The chat reference-style modals: CreateChannel / CreateCategory / EditChannel / ConfirmDelete
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
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
  useEffect(() => {
    if (!modal) return
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [modal, onClose])

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

function ModalShell({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  return (
    <div
      className="fc-modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="fc-modal">{children}</div>
    </div>
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
    <ModalShell onClose={onClose}>
      <h2>Create channel</h2>
      <p>In {categoryName}</p>
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
    <ModalShell onClose={onClose}>
      <h2>Create category</h2>
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
    <ModalShell onClose={onClose}>
      <h2>Edit channel</h2>
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
    <ModalShell onClose={onClose}>
      <h2>{title}</h2>
      <p>{description}</p>
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
