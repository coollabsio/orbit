// Server settings › Danger Zone: purge deleted messages + delete server.
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Danger } from 'reicon-react'
import { deleteWorkspaceContent, purgeDeletedMessages } from '../../mock/actions'
import { ConfirmDeleteModal } from '../chat/components/ChannelModals'
import './server.css'

export function DangerPage() {
  const navigate = useNavigate()
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [confirmPurge, setConfirmPurge] = useState(false)
  const [purging, setPurging] = useState(false)
  const [purgeResult, setPurgeResult] = useState<string | null>(null)

  function handlePurge() {
    setPurging(true)
    setPurgeResult(null)
    const count = purgeDeletedMessages()
    setPurgeResult(`Purged ${count} deleted message(s).`)
    setPurging(false)
  }

  return (
    <div className="fs-page">
      <div>
        <h2 className="fs-title" data-danger>
          Danger Zone
        </h2>
        <p className="fs-subtitle">These actions are irreversible. Please be certain.</p>
      </div>

      <div className="fs-danger-card">
        <div className="fs-danger-card-title">
          <Danger size={16} />
          <h3>Purge Deleted Messages</h3>
        </div>
        <p>Permanently remove all soft-deleted messages from this server.</p>
        <div className="fs-row">
          <button type="button" className="fs-btn" data-variant="danger" disabled={purging} onClick={() => setConfirmPurge(true)}>
            {purging ? 'Purging...' : 'Purge Deleted Messages'}
          </button>
          {purgeResult ? <span className="fs-success">{purgeResult}</span> : null}
        </div>
      </div>

      <div className="fs-danger-card">
        <div className="fs-danger-card-title" data-danger>
          <Danger size={16} />
          <h3>Delete Server</h3>
        </div>
        <p>This will permanently delete the server, all channels, and all messages.</p>
        <div className="fs-row">
          <button type="button" className="fs-btn" data-variant="danger" onClick={() => setConfirmDelete(true)}>
            Delete Server
          </button>
        </div>
      </div>

      {confirmPurge ? (
        <ConfirmDeleteModal
          title="Purge deleted messages?"
          description="This will permanently remove all soft-deleted messages from this server."
          onClose={() => setConfirmPurge(false)}
          onConfirm={() => {
            handlePurge()
            setConfirmPurge(false)
          }}
        />
      ) : null}
      {confirmDelete ? (
        <ConfirmDeleteModal
          title="Delete server?"
          description="This will permanently delete the server, all channels, and all messages."
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => {
            deleteWorkspaceContent()
            setConfirmDelete(false)
            navigate('/')
          }}
        />
      ) : null}
    </div>
  )
}
