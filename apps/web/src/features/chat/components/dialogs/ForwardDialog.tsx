import { useRef, useState, type ComponentProps } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useForwardMessage, useSendMessage } from '@/features/chat/api/mutations'
import { useConversations } from '@/features/chat/api/queries'
import { ChatError, MESSAGE_MAX_LENGTH, type Conversation, type Message } from '@/features/chat/api/types'
import { conversationTitle, useChatPeople } from '@/features/chat/components/messages/people'
import { MessageByline } from '@/features/chat/components/panes/MessageByline'
import { ConversationIcon } from '@/features/chat/components/sidebar/ConversationIcon'
import { replaceEmoticons } from '@/features/chat/lib/emoticons'
import { FORWARD_LIMIT, forwardDestinations, forwardToEach, toggleDestination } from '@/features/chat/lib/forward'
import { decodeMentions, encodeMentions } from '@/features/chat/lib/mentionTokens'
import { extractPreview, wordlessPreview } from '@/lib/messagePreview'

interface ForwardDialogProps {
  message: Message
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Where focus goes when the dialog closes: the message's row. */
  finalFocus?: ComponentProps<typeof DialogContent>['finalFocus']
}

/**
 * Forwards a message to up to five conversations: the channels the user can write in and the DMs, with a search field,
 * and an optional message of the user that is sent to each one right after the forward.
 */
export function ForwardDialog({ message, open, onOpenChange, finalFocus }: ForwardDialogProps) {
  const [pending, setPending] = useState(false)
  return (
    // While the forward runs the dialog stays: closing it would hide a failure.
    <Dialog open={open} onOpenChange={(next) => { if (next || !pending) onOpenChange(next) }}>
      <DialogContent finalFocus={finalFocus} className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Forward message</DialogTitle>
          <DialogDescription>Choose up to {FORWARD_LIMIT} conversations.</DialogDescription>
        </DialogHeader>
        <ForwardForm message={message} pending={pending} setPending={setPending} onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}

interface ForwardFormProps {
  message: Message
  /** The forward runs. The dialog holds it, because it must not close meanwhile. */
  pending: boolean
  setPending: (pending: boolean) => void
  onDone: () => void
}

function ForwardForm({ message, pending, setPending, onDone }: ForwardFormProps) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const conversations = useConversations().data ?? []
  const { mutateAsync: forwardMessage } = useForwardMessage()
  const { send } = useSendMessage()
  const [search, setSearch] = useState('')
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [comment, setComment] = useState('')
  const [error, setError] = useState<string | null>(null)
  // One nonce for each destination: a forward that goes again after a failure gives the first copy, not a second one.
  const nonces = useRef(new Map<string, string>())

  const titleOf = (conversation: Conversation) => conversationTitle(conversation, people, currentUserId)
  const destinations = forwardDestinations(conversations, titleOf, search)
  const text = extractPreview(decodeMentions(message.body, people.members, people.channels))
  const files = message.attachments.length

  function nonceFor(conversationId: string): string {
    const nonce = nonces.current.get(conversationId) ?? crypto.randomUUID()
    nonces.current.set(conversationId, nonce)
    return nonce
  }

  async function submit() {
    if (selectedIds.length === 0 || pending) return
    // The same stored form as a message from the composer.
    const body = encodeMentions(replaceEmoticons(comment.trim()), people.members, people.channels)
    setPending(true)
    setError(null)
    const result = await forwardToEach(
      selectedIds,
      (conversationId) => forwardMessage({ messageId: message.id, conversationId, nonce: nonceFor(conversationId) }),
      body ? (conversationId) => send({ conversationId, body }) : undefined,
    )
    setPending(false)
    const titles = result.sentIds.flatMap((id) => conversations.filter((conversation) => conversation.id === id).map(titleOf))
    if (result.sentIds.length === selectedIds.length) {
      onDone()
      toast.success(titles.length === 1 ? `Forwarded to ${titles[0]}` : `Forwarded to ${result.sentIds.length} conversations`)
      return
    }
    // The failed destination and those after it stay selected, so Send tries them again.
    const failed = conversations.find((conversation) => conversation.id === selectedIds[result.sentIds.length])
    const reason = (result.error instanceof ChatError && result.error.message) || 'The message was not forwarded. Try again.'
    setSelectedIds(selectedIds.filter((id) => !result.sentIds.includes(id)))
    setError(failed ? `${titleOf(failed)}: ${reason}` : reason)
  }

  return (
    <form
      className="grid min-w-0 gap-4"
      onSubmit={(event) => {
        event.preventDefault()
        void submit()
      }}
    >
      <div data-slot="forward-preview" className="grid min-w-0 gap-1 border-l-4 border-muted-foreground/40 pl-3">
        <MessageByline message={message} members={people.members} />
        {text ? <p className="line-clamp-2 text-[13px] wrap-anywhere text-muted-foreground">{text}</p> : null}
        {!text && message.stickerId ? <p className="text-[13px] text-muted-foreground">{wordlessPreview(true)}</p> : null}
        {files > 0 ? <p className="text-xs text-muted-foreground">{files === 1 ? '1 file' : `${files} files`}</p> : null}
      </div>
      <Command shouldFilter={false} label="Forward to" className="h-auto gap-1 bg-transparent p-0">
        <CommandInput autoFocus placeholder="Search channels and people…" value={search} onValueChange={setSearch} />
        <CommandList className="h-56 max-h-[40dvh]">
          <CommandEmpty>No conversations match</CommandEmpty>
          {destinations.map((conversation) => {
            const checked = selectedIds.includes(conversation.id)
            return (
              <CommandItem
                key={conversation.id}
                value={conversation.id}
                data-checked={checked}
                disabled={pending || (!checked && selectedIds.length >= FORWARD_LIMIT)}
                className="gap-2.5"
                onSelect={() => {
                  setSelectedIds(toggleDestination(selectedIds, conversation.id))
                  setError(null)
                }}
              >
                <ConversationIcon conversation={conversation} people={people.members} currentUserId={currentUserId} size={20} />
                <span className="truncate">{conversation.kind === 'dm' ? titleOf(conversation) : conversation.name}</span>
              </CommandItem>
            )
          })}
        </CommandList>
      </Command>
      <Input
        aria-label="Message"
        placeholder="Add an optional message"
        maxLength={MESSAGE_MAX_LENGTH}
        value={comment}
        disabled={pending}
        onChange={(event) => setComment(event.target.value)}
      />
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : null}
      <DialogFooter className="sm:items-center">
        <span className="text-xs text-muted-foreground sm:mr-auto">
          {selectedIds.length} of {FORWARD_LIMIT} selected
        </span>
        <DialogClose disabled={pending} render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="submit" disabled={selectedIds.length === 0 || pending}>
          {pending ? 'Sending…' : 'Send'}
        </Button>
      </DialogFooter>
    </form>
  )
}
