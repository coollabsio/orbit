import { useMemo, useState } from 'react'
import { MessagesSquare, Search } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Modal } from '@/components/common/Modal'
import { getOrCreateDirectMessage } from '@/mock/actions'
import type { AppState } from '@/mock/types'

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
      <div className="flex h-[38px] items-center gap-2 rounded-lg border border-input px-2.5 text-muted-foreground focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/20">
        <Search className="size-4 shrink-0" />
        <Input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search by name or username…" autoFocus className="h-auto min-w-0 flex-1 rounded-none border-0 bg-transparent p-0 text-sm text-foreground shadow-none outline-none focus-visible:ring-0 md:text-sm dark:bg-transparent" />
      </div>
      <div className="mt-3 max-h-80 overflow-y-auto rounded-[10px] border border-border [&>button+button]:border-t [&>button+button]:border-border">
        {users.length > 0 ? users.map((user) => (
          <Button
            key={user.id}
            type="button"
            variant="ghost"
            className="flex h-auto w-full items-center justify-start gap-3 rounded-none border-0 px-3 py-2.5 text-left font-normal whitespace-normal transition-colors hover:bg-muted active:not-aria-[haspopup]:translate-y-0 dark:hover:bg-muted"
            onClick={() => onCreated(getOrCreateDirectMessage(user.id))}
          >
            <UserAvatar user={user} size={36} />
            <span className="flex min-w-0 flex-col">
              <strong className="text-[13px] text-foreground">{user.name}</strong>
              <small className="text-[11px] text-muted-foreground/70">@{user.handle}</small>
            </span>
          </Button>
        )) : <div className="flex items-center justify-center gap-2 p-7 text-[13px] text-muted-foreground/70"><MessagesSquare className="size-5" />No matching people</div>}
      </div>
    </Modal>
  )
}
