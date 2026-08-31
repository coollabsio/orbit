import { useMemo, useState } from 'react'
import { Magnifier, Messages2 } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Modal } from '../../../components/ui/Modal'
import { getOrCreateDirectMessage } from '../../../mock/actions'
import type { AppState } from '../../../mock/types'

export function NewDMModal({ state, onClose, onCreated }: { state: AppState; onClose: () => void; onCreated: (id: string) => void }) {
  const [query, setQuery] = useState('')
  const users = useMemo(() => {
    const search = query.trim().toLowerCase()
    return state.users
      .filter((user) => user.id !== state.currentUserId)
      .filter((user) => !search || user.name.toLowerCase().includes(search) || user.handle.toLowerCase().includes(search))
  }, [query, state.currentUserId, state.users])

  return (
    <Modal title="New message" description="Select a person to message" onClose={onClose} maxWidth={448}>
      <div className="dm-search">
        <Magnifier size={15} />
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search by name or username…" autoFocus />
      </div>
      <div className="dm-member-list">
        {users.length > 0 ? users.map((user) => (
          <button
            key={user.id}
            type="button"
            className="dm-member-row"
            onClick={() => onCreated(getOrCreateDirectMessage(user.id))}
          >
            <Avatar user={user} size={36} />
            <span className="dm-member-copy">
              <strong>{user.name}</strong>
              <small>@{user.handle}</small>
            </span>
          </button>
        )) : <div className="dm-empty"><Messages2 size={20} />No matching people</div>}
      </div>
    </Modal>
  )
}
