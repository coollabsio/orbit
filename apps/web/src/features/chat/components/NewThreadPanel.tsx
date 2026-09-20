// Port of the chat reference NewThreadPanel: title input above a composer; sending creates the
// thread-starter root plus the first reply, then opens the thread.
import { useId, useRef, useState } from 'react'
import { Xmark as X } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import { createThread } from '@/mock/actions'
import type { AppState, Channel } from '@/mock/types'
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
  const titleId = useId()

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
        <Button type="button" variant="ghost" size="icon-sm" className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground dark:hover:bg-muted" title="Close new thread" aria-label="Close new thread" onClick={onClose}>
          <X size={16} />
        </Button>
      </div>
      <div className="flex-1" />
      <div className="flex flex-col gap-4 px-3 pb-4">
        <div className="flex size-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
          <ThreadIcon size={28} />
        </div>
        <div>
          <Label htmlFor={titleId} className="mb-2 block text-sm leading-normal font-semibold text-foreground">
            Thread Name
          </Label>
          <Input
            ref={titleRef}
            id={titleId}
            className="h-10 w-full rounded-lg border border-input bg-background px-3 text-sm font-semibold text-foreground placeholder:font-medium placeholder:text-muted-foreground focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/10 md:text-sm dark:bg-background"
            value={title}
            placeholder="New Thread"
            autoFocus
            onChange={(e) => setTitle(e.target.value)}
          />
        </div>
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
