import { useMemo, useState } from 'react'
import { Messages2 as MessagesSquare, SearchNormal as Search } from 'reicon-react'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Modal } from '@/components/common/Modal'
import { getOrCreateDirectMessage } from '@/mock/actions'
import type { AppState } from '@/mock/types'
import { RowButton } from './RowButton'

export function NewDMModal({ state, onClose, onCreated }: { state: AppState; onClose: () => void; onCreated: (id: string) => void }) {
  const [query, setQuery] = useState('')
  const users = useMemo(() => {
    const search = query.trim().toLowerCase()
    return state.users
      .filter((user) => user.id !== state.currentUserId)
      .filter((user) => !search || user.name.toLowerCase().includes(search) || user.handle.toLowerCase().includes(search))
  }, [query, state.currentUserId, state.users])

  return (
    <Modal title="New message" description="Select a person to message" onClose={onClose} className="sm:max-w-md">
      <InputGroup>
        <InputGroupAddon>
          <Search />
        </InputGroupAddon>
        <InputGroupInput value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search by name or username…" aria-label="Search people" autoFocus />
      </InputGroup>
      <div className="max-h-80 overflow-y-auto rounded-[10px] border border-border [&>button+button]:border-t [&>button+button]:border-border">
        {users.length > 0 ? users.map((user) => (
          <RowButton
            key={user.id}
            className="gap-3 px-3 py-2.5 hover:bg-muted"
            onClick={() => onCreated(getOrCreateDirectMessage(user.id))}
          >
            <UserAvatar user={user} size={36} />
            <span className="flex min-w-0 flex-col">
              <strong className="text-[13px] text-foreground">{user.name}</strong>
              <small className="text-[11px] text-muted-foreground/70">@{user.handle}</small>
            </span>
          </RowButton>
        )) : <div className="flex items-center justify-center gap-2 p-7 text-[13px] text-muted-foreground/70"><MessagesSquare className="size-5" />No matching people</div>}
      </div>
    </Modal>
  )
}
