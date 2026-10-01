import { useChatContext } from '../../api/chatContext'
import { useConnectionStatus, useTyping } from '../../api/liveStore'
import { typingLabel } from '../messages/messageText'
import { useChatPeople } from '../messages/people'

/**
 * Who is typing: up to two names, then "Several people are typing". The line keeps its height and only its opacity
 * changes, so the composer does not move.
 */
export function TypingLine({ conversationId, threadRootId = null }: { conversationId: string; threadRootId?: string | null }) {
  const { currentUserId } = useChatContext()
  const people = useChatPeople()
  const typing = useTyping(conversationId, threadRootId)
  const names = typing.filter((id) => id !== currentUserId).map((id) => people.byId.get(id)?.name ?? 'Someone')
  const label = typingLabel(names)
  return (
    <div
      data-slot="typing-line"
      data-active={label ? '' : undefined}
      className="h-5 shrink-0 truncate px-4 text-xs leading-5 text-muted-foreground opacity-0 transition-opacity duration-150 ease-out data-active:opacity-100 max-[899px]:px-3"
    >
      {label ? `${label}…` : null}
    </div>
  )
}

/** Shown above the composer while the connection is down. Sending stays possible: messages wait as "Sending". */
export function ConnectionLine() {
  const status = useConnectionStatus()
  if (status === 'connected') return null
  return (
    <div role="status" data-slot="connection-line" className="shrink-0 px-4 pb-1 text-xs text-muted-foreground max-[899px]:px-3">
      Reconnecting… Messages you send now are sent when you are back online.
    </div>
  )
}
