// Port of the chat reference NewThreadPanel: title input above a composer; sending creates the
// thread-starter root plus the first reply, then opens the thread.
import { useRef, useState } from 'react'
import { X } from 'lucide-react'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import { createThread } from '../../../mock/actions'
import type { AppState, Channel } from '../../../mock/types'
import { MessageInput } from './MessageInput'

export function NewThreadPanel({
  state,
  channel,
  onClose,
  onCreated,
}: {
  state: AppState
  channel: Channel
  onClose: () => void
  onCreated: (rootId: string) => void
}) {
  const [title, setTitle] = useState('')
  const titleRef = useRef<HTMLInputElement>(null)

  function handleSend(content: string) {
    const trimmedTitle = title.trim()
    if (!trimmedTitle) {
      titleRef.current?.focus()
      return false
    }
    onCreated(createThread(channel.id, trimmedTitle, content))
    return true
  }

  return (
    <div className="relative flex h-full w-96 shrink-0 flex-col border-l border-border bg-background">
      <div className="flex h-[47px] shrink-0 items-center justify-between border-b border-border px-4">
        <div className="flex min-w-0 flex-1 items-center gap-2 text-muted-foreground">
          <ThreadIcon size={20} />
          <h2 className="min-w-0 text-[15px] font-semibold text-foreground">New Thread</h2>
        </div>
        <button type="button" className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" title="Close new thread" aria-label="Close new thread" onClick={onClose}>
          <X size={16} />
        </button>
      </div>
      <div className="flex-1" />
      <div className="flex flex-col gap-4 px-3 pb-4">
        <div className="flex size-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
          <ThreadIcon size={28} />
        </div>
        <label className="block">
          <span className="mb-2 block text-sm font-semibold text-foreground">Thread Name</span>
          <input
            ref={titleRef}
            className="h-10 w-full rounded-lg border border-input bg-background px-3 text-sm font-semibold text-foreground placeholder:font-medium placeholder:text-muted-foreground focus:border-primary focus:ring-2 focus:ring-primary/10 focus:outline-none"
            value={title}
            placeholder="New Thread"
            autoFocus
            onChange={(e) => setTitle(e.target.value)}
          />
        </label>
      </div>
      <MessageInput
        state={state}
        channel={channel}
        placeholder="Enter a message to start the conversation!"
        showThreadAction={false}
        onSend={handleSend}
      />
    </div>
  )
}
